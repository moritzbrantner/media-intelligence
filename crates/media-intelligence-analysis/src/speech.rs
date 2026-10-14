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
//!   * enabled forced alignment with neither `model_bundle` nor `model_cache_only`.
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
//!   the VAD, alignment and diarization options. It does not cover the
//!   [`ModelFetchPolicy`] (a permission, not an option) or the declared model version
//!   (already a separate identity field).
//! * **Run.** [`SpeechProducer::run`] checks cancellation first and returns
//!   [`ProducerError::Cancelled`] without touching audio-analysis or the file system.
//!   It then checks every explicitly configured local model location (Candle
//!   `model_bundle`/`model_dir`, whisper.cpp `model_path`, which must be present, WhisperX
//!   `model_dir`, alignment `model_bundle`/`model_dir`). A missing location is
//!   [`ProducerError::Failed`] with a reason naming the option and the path, before
//!   audio-analysis is called. Otherwise it calls `transcribe` with
//!   [`AudioAnalysisSpeechProducer::transcription_request`] and maps the response with
//!   [`speech_run_from_response`]. Pipeline errors are [`ProducerError::Failed`] carrying
//!   the upstream message.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The pinned audio-analysis transcription crate, so callers build provider options with
/// exactly the types this adapter accepts.
pub use audio_analysis_transcription as transcription;
pub use audio_analysis_transcription::{
    AlignmentOptions, CandleWhisperOptions, DiarizationOptions, TranscriptionPipelineRequest,
    TranscriptionPipelineResponse, TranscriptionProviderSelection, TranscriptionSource, VadOptions,
    WhisperCppProviderOptions, WhisperXCommandOptions,
};

use crate::{ProducerError, ProducerIdentity, ProducerInput, SpeechProducer, SpeechRun};

/// Exact `audio-analysis` revision whose transcription surface this adapter invokes.
pub const AUDIO_ANALYSIS_REVISION: &str = "cc8075f633a4bda5d1b9902b3f4c4d07eca017d9";
/// Prefix of the receipt `producer` field: `audio-analysis:<provider id>`.
pub const SPEECH_PRODUCER_PREFIX: &str = "audio-analysis";
/// Receipt `model_version` when the caller declares no model version.
pub const UNSPECIFIED_MODEL_VERSION: &str = "unspecified";

/// Whether `audio-analysis` may fetch models over the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelFetchPolicy {
    /// Local models only (the default). Download-capable configurations are refused.
    #[default]
    LocalOnly,
    /// The caller explicitly allows first-run model downloads and network access.
    AllowNetwork,
}

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
        let _ = provider;
        todo!("implementation pending (#32)")
    }

    /// Deterministic `sha256:<64 hex>` of the effective transcription options (see the
    /// module documentation for what it covers).
    pub fn config_hash(&self) -> String {
        todo!("implementation pending (#32)")
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
        let _ = config;
        todo!("implementation pending (#32)")
    }

    pub fn config(&self) -> &AudioAnalysisSpeechConfig {
        &self.config
    }

    /// The exact request [`SpeechProducer::run`] passes to audio-analysis for the media at
    /// `media_path`: source `TranscriptionSource::Path { media_path }`, and the configured
    /// provider, VAD, alignment and diarization options.
    pub fn transcription_request(&self, media_path: &Path) -> TranscriptionPipelineRequest {
        let _ = media_path;
        todo!("implementation pending (#32)")
    }
}

impl SpeechProducer for AudioAnalysisSpeechProducer {
    fn identity(&self) -> ProducerIdentity {
        todo!("implementation pending (#32)")
    }

    fn run(&self, input: &ProducerInput<'_>) -> Result<SpeechRun, ProducerError> {
        let _ = input;
        todo!("implementation pending (#32)")
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
    let _ = (identity, input_hash, response);
    todo!("implementation pending (#32)")
}
