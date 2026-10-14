//! `audio-analysis` speech producer adapter (#32), behind the opt-in `audio-analysis`
//! feature.
//!
//! [`AudioAnalysisSpeechProducer`] implements [`SpeechProducer`] on top of the public
//! transcription surface of `audio-analysis` (`audio_analysis_transcription::transcribe`
//! with a [`TranscriptionPipelineRequest`]), pinned to [`AUDIO_ANALYSIS_REVISION`]. The
//! caller chooses the provider ([`TranscriptionProviderSelection`]) and supplies the model
//! location. This module contains no ASR, model-runtime or decoding code.
//!
//! # Contract fixed by the acceptance tests (#32)
//!
//! * **No implicit downloads.** [`AudioAnalysisSpeechProducer::new`] refuses, with
//!   [`SpeechAdapterError::ModelFetchNotAllowed`], every configuration under which
//!   `audio-analysis` could fetch a model over the network, unless the caller sets
//!   [`ModelFetchPolicy::AllowNetwork`]. Download-capable means:
//!   * Candle Whisper with neither `model_bundle` nor `model_cache_only`;
//!   * external WhisperX without `model_cache_only`;
//!   * enabled forced alignment with neither `model_bundle` nor `model_cache_only`;
//!   * external WhisperX with alignment enabled (`no_align = false`) or `diarize`: its
//!     torchaudio/NLTK alignment and pyannote diarization ignore `model_cache_only`;
//!   * external WhisperX with any `extra_args` (free-form flags such as
//!     `--vad_method silero`, which loads through torch.hub, cannot be vetted).
//!
//!   whisper.cpp (explicit `model_path`) and diarization (local bundles) never download.
//!   The refusal happens at construction, before anything runs.
//! * **Identity.** `producer` = `audio-analysis:<provider id>` (the provider id from
//!   [`TranscriptionProviderSelection::provider_id`]), `producer_version` =
//!   [`AUDIO_ANALYSIS_REVISION`], `model` = the selection's model id, `model_version` = the
//!   caller-declared [`AudioAnalysisSpeechConfig::model_version`] or
//!   [`UNSPECIFIED_MODEL_VERSION`], `config_hash` = [`AudioAnalysisSpeechConfig::config_hash`].
//! * **Config hash.** A deterministic `sha256:<64 hex>` over the effective transcription
//!   options: the provider selection with all its options (including model locations), and
//!   the VAD, alignment and diarization options; a disabled VAD, alignment or diarization
//!   stage contributes only its `enabled` flag. It does not cover the
//!   [`ModelFetchPolicy`] (a permission, not an option) or the declared model version
//!   (already a separate identity field).
//! * **Run.** [`SpeechProducer::run`] checks cancellation first and returns
//!   [`ProducerError::Cancelled`] without touching audio-analysis or the file system.
//!   It then checks the local model location audio-analysis will actually read (Candle
//!   `model_bundle`, else `model_dir`; whisper.cpp `model_path`, which must be present;
//!   WhisperX `model_dir`; enabled alignment `model_bundle`, else `model_dir`). A missing location is
//!   [`ProducerError::Failed`] with a reason naming the option and the path, before
//!   audio-analysis is called. Otherwise it calls `transcribe` with
//!   [`AudioAnalysisSpeechProducer::transcription_request`] and maps the response with
//!   [`speech_run_from_response`]. Pipeline errors are [`ProducerError::Failed`] carrying
//!   the upstream message.

use std::path::Path;

use serde_json::json;

/// The pinned audio-analysis transcription crate, so callers build provider options with
/// exactly the types this adapter accepts.
pub use audio_analysis_transcription as transcription;
pub use audio_analysis_transcription::{
    AlignmentOptions, CandleWhisperOptions, DiarizationOptions, TranscriptionPipelineRequest,
    TranscriptionPipelineResponse, TranscriptionProviderSelection, TranscriptionSource, VadOptions,
    WhisperCppProviderOptions, WhisperXCommandOptions,
};

use crate::{
    execution::sha256_prefixed, Modality, ProducerError, ProducerIdentity, ProducerInput,
    ProducerReceipt, SpeechProducer, SpeechRun,
};

/// Exact `audio-analysis` revision whose transcription surface this adapter invokes.
pub const AUDIO_ANALYSIS_REVISION: &str = "cc8075f633a4bda5d1b9902b3f4c4d07eca017d9";
/// Prefix of the receipt `producer` field: `audio-analysis:<provider id>`.
pub const SPEECH_PRODUCER_PREFIX: &str = "audio-analysis";
/// Receipt `model_version` when the caller declares no model version.
pub const UNSPECIFIED_MODEL_VERSION: &str = "unspecified";

/// Whether `audio-analysis` may fetch models over the network (shared with the #33
/// visible-text adapter; defined at the crate root).
pub use crate::ModelFetchPolicy;

/// The caller's transcription configuration for one [`AudioAnalysisSpeechProducer`].
#[derive(Debug, Clone, PartialEq)]
pub struct AudioAnalysisSpeechConfig {
    /// Provider and its options, including the model location.
    pub provider: TranscriptionProviderSelection,
    pub vad: VadOptions,
    pub alignment: AlignmentOptions,
    pub diarization: DiarizationOptions,
    /// Caller-declared model version (for example a model revision or bundle digest).
    /// audio-analysis does not report one.
    pub model_version: Option<String>,
    pub model_fetch: ModelFetchPolicy,
}

impl AudioAnalysisSpeechConfig {
    /// `provider` with default VAD, alignment (disabled) and diarization (disabled)
    /// options, no declared model version and [`ModelFetchPolicy::LocalOnly`].
    pub fn new(provider: TranscriptionProviderSelection) -> Self {
        Self {
            provider,
            vad: VadOptions::default(),
            alignment: AlignmentOptions::default(),
            diarization: DiarizationOptions::default(),
            model_version: None,
            model_fetch: ModelFetchPolicy::LocalOnly,
        }
    }

    /// Deterministic `sha256:<64 hex>` of the effective transcription options (see the
    /// module documentation for what it covers).
    pub fn config_hash(&self) -> String {
        // serde_json writes struct fields in declaration order, so equal options always
        // serialize identically.
        // A disabled stage contributes only its `enabled` flag: settings it would ignore
        // cannot change the transcript, so they must not invalidate cached runs.
        let stage = |enabled: bool, options: serde_json::Value| {
            if enabled {
                options
            } else {
                json!({ "enabled": false })
            }
        };
        let material = json!({
            "provider": &self.provider,
            "vad": stage(self.vad.enabled, json!(&self.vad)),
            "alignment": stage(self.alignment.enabled, json!(&self.alignment)),
            "diarization": stage(self.diarization.enabled, json!(&self.diarization)),
        });
        sha256_prefixed(
            serde_json::to_string(&material)
                .expect("transcription options serialize")
                .as_bytes(),
        )
    }

    /// The download-capable option, if any (see the module documentation).
    fn download_capable_option(&self) -> Option<String> {
        match &self.provider {
            TranscriptionProviderSelection::CandleWhisper(options)
                if options.model_bundle.is_none() && !options.model_cache_only =>
            {
                return Some(
                    "Candle Whisper without `model_bundle` or `model_cache_only` may download \
                     the model; supply a local `model_bundle` or set `model_cache_only` with a \
                     populated `model_dir`"
                        .to_string(),
                );
            }
            TranscriptionProviderSelection::ExternalWhisperX(options)
                if !options.model_cache_only =>
            {
                return Some(
                    "external WhisperX without `model_cache_only` may download models; set \
                     `model_cache_only` with a populated `model_dir`"
                        .to_string(),
                );
            }
            TranscriptionProviderSelection::ExternalWhisperX(options)
                if !options.extra_args.is_empty() =>
            {
                // Free-form WhisperX flags (for example `--vad_method silero`, which loads
                // through torch.hub) can enable network-capable paths this adapter cannot
                // vet.
                return Some(
                    "external WhisperX `extra_args` can select network-capable components \
                     (for example `--vad_method silero` via torch.hub); remove them or allow \
                     network access"
                        .to_string(),
                );
            }
            TranscriptionProviderSelection::ExternalWhisperX(options) if !options.no_align => {
                // WhisperX loads torchaudio alignment bundles and NLTK data without honouring
                // `model_cache_only`, so its alignment can still reach the network.
                return Some(
                    "external WhisperX alignment may download torchaudio/NLTK alignment data \
                     even with `model_cache_only`; set `no_align` (use the local audio-analysis \
                     alignment options instead)"
                        .to_string(),
                );
            }
            TranscriptionProviderSelection::ExternalWhisperX(options) if options.diarize => {
                // WhisperX applies cache-only to ASR and alignment but not to its pyannote
                // diarization pipeline, which can still fetch from Hugging Face.
                return Some(
                    "external WhisperX `diarize` may download the pyannote diarization model \
                     even with `model_cache_only`; disable `diarize` (use the local \
                     audio-analysis diarization options instead)"
                        .to_string(),
                );
            }
            _ => {}
        }
        if self.alignment.enabled
            && self.alignment.model_bundle.is_none()
            && !self.alignment.model_cache_only
        {
            return Some(
                "forced alignment without `model_bundle` or `model_cache_only` may download the \
                 alignment model; supply a local `model_bundle` or set `model_cache_only` with a \
                 populated `model_dir`"
                    .to_string(),
            );
        }
        None
    }

    /// The first explicitly configured local model location that does not exist (or a
    /// required one that is not configured), as an actionable reason.
    fn missing_model_location(&self) -> Option<String> {
        let mut locations: Vec<(&str, Option<&Path>)> = Vec::new();
        match &self.provider {
            // audio-analysis resolves a bundle before a model directory, so only the
            // location it will actually read is checked.
            TranscriptionProviderSelection::CandleWhisper(options) => {
                match options.model_bundle.as_deref() {
                    Some(bundle) => locations.push(("candle model_bundle", Some(bundle))),
                    None => locations.push(("candle model_dir", options.model_dir.as_deref())),
                }
            }
            TranscriptionProviderSelection::WhisperCpp(options) => {
                let Some(path) = options.model_path.as_deref() else {
                    return Some(
                        "whisper.cpp `model_path` is required but not configured".to_string(),
                    );
                };
                locations.push(("whisper.cpp model_path", Some(path)));
            }
            TranscriptionProviderSelection::ExternalWhisperX(options) => {
                locations.push(("whisperx model_dir", options.model_dir.as_deref()));
            }
        }
        if self.alignment.enabled {
            match self.alignment.model_bundle.as_deref() {
                Some(bundle) => locations.push(("alignment model_bundle", Some(bundle))),
                None => {
                    locations.push(("alignment model_dir", self.alignment.model_dir.as_deref()))
                }
            }
        }
        locations.into_iter().find_map(|(option, path)| {
            let path = path?;
            (!path.exists()).then(|| {
                format!(
                    "configured {option} `{}` does not exist; provide the model locally",
                    path.display()
                )
            })
        })
    }
}

/// Why an [`AudioAnalysisSpeechProducer`] could not be constructed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SpeechAdapterError {
    /// The configuration could make audio-analysis download a model, and the caller did
    /// not set [`ModelFetchPolicy::AllowNetwork`]. `reason` names the download-capable
    /// option and how to make it local (a model bundle or `model_cache_only`).
    ModelFetchNotAllowed { reason: String },
}

impl std::fmt::Display for SpeechAdapterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for SpeechAdapterError {}

/// [`SpeechProducer`] backed by `audio-analysis` transcription.
#[derive(Debug, Clone)]
pub struct AudioAnalysisSpeechProducer {
    config: AudioAnalysisSpeechConfig,
}

impl AudioAnalysisSpeechProducer {
    /// Validates `config`. Refuses download-capable configurations unless
    /// `config.model_fetch` is [`ModelFetchPolicy::AllowNetwork`]. Does not touch the file
    /// system or the network.
    pub fn new(config: AudioAnalysisSpeechConfig) -> Result<Self, SpeechAdapterError> {
        if config.model_fetch == ModelFetchPolicy::LocalOnly {
            if let Some(reason) = config.download_capable_option() {
                return Err(SpeechAdapterError::ModelFetchNotAllowed { reason });
            }
        }
        Ok(Self { config })
    }

    pub fn config(&self) -> &AudioAnalysisSpeechConfig {
        &self.config
    }

    /// The exact request [`SpeechProducer::run`] passes to audio-analysis for the media at
    /// `media_path`: source `TranscriptionSource::Path { media_path }`, and the configured
    /// provider, VAD, alignment and diarization options.
    pub fn transcription_request(&self, media_path: &Path) -> TranscriptionPipelineRequest {
        TranscriptionPipelineRequest {
            source: TranscriptionSource::Path {
                path: media_path.to_path_buf(),
            },
            provider: self.config.provider.clone(),
            vad: self.config.vad.clone(),
            alignment: self.config.alignment.clone(),
            diarization: self.config.diarization.clone(),
            output: Default::default(),
        }
    }
}

impl SpeechProducer for AudioAnalysisSpeechProducer {
    fn identity(&self) -> ProducerIdentity {
        let provider = &self.config.provider;
        ProducerIdentity {
            producer: format!("{SPEECH_PRODUCER_PREFIX}:{}", provider.provider_id()),
            producer_version: AUDIO_ANALYSIS_REVISION.to_string(),
            model: provider.model_id().to_string(),
            model_version: self
                .config
                .model_version
                .clone()
                .unwrap_or_else(|| UNSPECIFIED_MODEL_VERSION.to_string()),
            config_hash: self.config.config_hash(),
        }
    }

    fn run(&self, input: &ProducerInput<'_>) -> Result<SpeechRun, ProducerError> {
        if input.cancellation.is_cancelled() {
            return Err(ProducerError::Cancelled);
        }
        if let Some(reason) = self.config.missing_model_location() {
            return Err(ProducerError::Failed { reason });
        }
        let response =
            audio_analysis_transcription::transcribe(self.transcription_request(input.media_path))
                .map_err(|error| ProducerError::Failed {
                    reason: format!("audio-analysis transcription failed: {error}"),
                })?;
        speech_run_from_response(&self.identity(), input.input_hash, &response)
    }
}

/// Maps a recorded audio-analysis response into a [`SpeechRun`] (pure; no I/O).
///
/// The receipt is [`crate::Modality::Speech`] with every `identity` field and `input_hash`.
/// Segments come from `response.transcript` through [`SpeechRun::from_timed_text`], with
/// exact times, text and confidence. Fails with [`ProducerError::Failed`] when the
/// response is not `accepted`, when its `provider` or `model_id` disagree with `identity`
/// (`producer` = `audio-analysis:<provider>`, `model` = `model_id`), or when a segment has
/// no time range.
pub fn speech_run_from_response(
    identity: &ProducerIdentity,
    input_hash: &str,
    response: &TranscriptionPipelineResponse,
) -> Result<SpeechRun, ProducerError> {
    let failed = |reason: String| ProducerError::Failed { reason };
    if !response.accepted {
        return Err(failed(format!(
            "audio-analysis did not accept the transcription: {}",
            response.diagnostics.join("; ")
        )));
    }
    let producer = format!("{SPEECH_PRODUCER_PREFIX}:{}", response.provider);
    if producer != identity.producer {
        return Err(failed(format!(
            "audio-analysis answered with provider `{}`, expected `{}`",
            producer, identity.producer
        )));
    }
    if response.model_id != identity.model {
        return Err(failed(format!(
            "audio-analysis answered with model `{}`, expected `{}`",
            response.model_id, identity.model
        )));
    }
    let receipt = ProducerReceipt {
        modality: Modality::Speech,
        producer: identity.producer.clone(),
        producer_version: identity.producer_version.clone(),
        model: identity.model.clone(),
        model_version: identity.model_version.clone(),
        config_hash: identity.config_hash.clone(),
        input_hash: input_hash.to_string(),
        run_id: None,
        config: None,
    };
    SpeechRun::from_timed_text(receipt, &response.transcript).map_err(|error| {
        failed(format!(
            "transcript cannot be placed on the timeline: {error}"
        ))
    })
}
