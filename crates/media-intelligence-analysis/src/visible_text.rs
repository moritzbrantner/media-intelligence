//! `visual-analysis` visible-text producer adapter (#33), behind the opt-in
//! `visual-analysis-ocr` feature.
//!
//! [`VisualAnalysisOcrProducer`] implements [`VisibleTextProducer`] on top of the canonical
//! visual-analysis path, pinned to [`VISUAL_ANALYSIS_REVISION`]:
//! `video_analysis_ingest::surface::scene_ocr::analyze_scene_aware_ocr` samples
//! representative frames from the scene intervals of the same analysis, runs the
//! caller-selected `image-analysis-ocr` backend on them, and builds text tracks with
//! `video_analysis_recognition::analyze_video_text_semantics`. This module contains no
//! frame sampling, OCR, tracking or role-classification code; it configures the upstream
//! call and maps its output.
//!
//! # Contract fixed by the acceptance tests (#33)
//!
//! * **Scene dependency.** [`VisibleTextProducer::requires_scenes`] is `true`. The adapter
//!   OCRs only the scene intervals of [`ProducerInput::scenes`]. Without a scene run it
//!   returns [`ProducerError::Failed`] with a reason that names the scene stage and how to
//!   request it; it never detects scenes itself. Its receipts name
//!   [`crate::scene_bound_identity`] of [`VisibleTextProducer::identity`] and that scene run.
//! * **No implicit downloads.** [`VisualAnalysisOcrProducer::new`] refuses, with
//!   [`OcrAdapterError::ModelFetchNotAllowed`], every backend configuration under which
//!   `image-analysis-ocr` could fetch a model over the network (ONNX TrOCR with
//!   `auto_download`), unless the caller sets [`ModelFetchPolicy::AllowNetwork`]. The JSON
//!   external-command backend runs the caller's own executable and is never refused.
//! * **Identity.** `producer` = `visual-analysis:<backend kind>` ([`OCR_PRODUCER_PREFIX`],
//!   [`OcrBackendConfig::kind`]), `producer_version` = [`VISUAL_ANALYSIS_REVISION`],
//!   `model` = [`OcrBackendConfig::model_id`], `model_version` = the caller-declared
//!   [`VisualAnalysisOcrConfig::model_version`] or [`UNSPECIFIED_MODEL_VERSION`],
//!   `config_hash` = [`VisualAnalysisOcrConfig::config_hash`].
//! * **Config hash.** A deterministic `sha256:<64 hex>` over the backend kind and its
//!   result-relevant options (ONNX: preset, `bundle_root`, `cache_dir`; JSON command: name,
//!   command, args) and the full [`OcrRequest`]. It does not cover the [`ModelFetchPolicy`],
//!   the declared model version, or the ONNX download settings (`auto_download`,
//!   `download_progress`, `hf_token`, `max_retries`). Upstream frame sampling has no
//!   options at the pinned revision; the scene intervals enter through
//!   [`crate::scene_bound_identity`].
//! * **Run.** [`VisibleTextProducer::run`] checks cancellation first and returns
//!   [`ProducerError::Cancelled`] without touching the file system or visual-analysis. It
//!   then requires the scene run, checks a configured ONNX `bundle_root` exists (a missing
//!   one is [`ProducerError::Failed`] naming `bundle_root` and the path), opens the media
//!   with `FfmpegVideoSource`, calls `analyze_scene_aware_ocr` with the scene intervals,
//!   and maps the result with [`visible_text_run_from_analysis`]. Upstream errors are
//!   [`ProducerError::Failed`] carrying the upstream message.
//! * **Mapping** ([`visible_text_run_from_analysis`], pure): one [`OcrObservation`] per
//!   upstream text observation, with a run-unique id, the text, the frame time and index,
//!   the `language` attribute, the region and the upstream score as confidence. One
//!   [`OcrTrack`] per upstream `VideoTextTrack`, with its id, consensus text, role (the
//!   upstream serde name, for example `presentation_slide`) and exactly its member
//!   observations, as the producer grouped them. The core collapses repeated frames only
//!   through these tracks; this module never groups observations itself.
//!
//! [`OcrObservation`]: media_intelligence_core::OcrObservation
//! [`OcrTrack`]: media_intelligence_core::OcrTrack

use std::collections::BTreeSet;
use std::path::PathBuf;

use media_intelligence_core::{OcrObservation, OcrTrack, Region};
use num_rational::Rational64;
use serde_json::json;
use video_analysis_core::{FramePosition, Scene};
use video_analysis_ffmpeg::FfmpegVideoSource;
use video_analysis_ingest::surface::scene_ocr::analyze_scene_aware_ocr;

/// The pinned `image-analysis-ocr` crate (backends, [`OcrRequest`], presets), so callers
/// build backend options with exactly the types this adapter accepts.
pub use image_analysis_ocr as ocr;
pub use image_analysis_ocr::{OcrLocalModelOptions, OcrPreset, OcrRequest};
/// The pinned `video-analysis-core` crate (observations, frame positions, scenes).
pub use video_analysis_core as video_core;
/// The upstream scene-aware OCR output this adapter maps.
pub use video_analysis_ingest::surface::scene_ocr::SceneAwareOcrAnalysis;
/// The pinned `video-analysis-recognition` crate (text tracks and semantics).
pub use video_analysis_recognition as recognition;

use crate::execution::sha256_prefixed;
pub use crate::ModelFetchPolicy;
pub use crate::VISUAL_ANALYSIS_REVISION;
use crate::{
    scene_bound_identity, Modality, ProducerError, ProducerIdentity, ProducerInput,
    ProducerReceipt, SceneRun, VisibleTextProducer, VisibleTextRun,
};

/// Prefix of the receipt `producer` field: `visual-analysis:<backend kind>`.
pub const OCR_PRODUCER_PREFIX: &str = "visual-analysis";
/// Backend kind of [`OcrBackendConfig::OnnxTrOcr`].
pub const ONNX_TROCR_KIND: &str = "onnx-trocr";
/// Backend kind of [`OcrBackendConfig::JsonCommand`].
pub const JSON_COMMAND_KIND: &str = "json-command";
/// Receipt `model_version` when the caller declares no model version.
pub const UNSPECIFIED_MODEL_VERSION: &str = "unspecified";

/// The caller-selected `image-analysis-ocr` backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrBackendConfig {
    /// `image_analysis_ocr::OnnxTrOcrBackend::from_local_model` with these options. Real
    /// inference needs the `visual-analysis-ocr-onnx` feature. Download-capable when
    /// `auto_download` is set.
    OnnxTrOcr(OcrLocalModelOptions),
    /// `image_analysis_ocr::JsonCommandOcrBackend::new(command).args(args)`: the command
    /// receives each sampled frame as a PNG and prints an `OcrDocument` JSON object.
    JsonCommand {
        /// Caller-chosen identifier of the command and the model behind it (the receipt
        /// `model`), for example `tesseract-5-eng`.
        name: String,
        command: PathBuf,
        args: Vec<String>,
    },
}

impl OcrBackendConfig {
    /// [`ONNX_TROCR_KIND`] or [`JSON_COMMAND_KIND`].
    pub fn kind(&self) -> &'static str {
        match self {
            Self::OnnxTrOcr(_) => ONNX_TROCR_KIND,
            Self::JsonCommand { .. } => JSON_COMMAND_KIND,
        }
    }

    /// The receipt `model`: the preset's Hugging Face repo id
    /// ([`OcrPreset::repo_id`]) for ONNX TrOCR, the caller's `name` for a JSON command.
    pub fn model_id(&self) -> String {
        match self {
            Self::OnnxTrOcr(options) => options.preset.repo_id().to_string(),
            Self::JsonCommand { name, .. } => name.clone(),
        }
    }

    /// Result-relevant backend options (never download settings or credentials).
    fn hash_material(&self) -> serde_json::Value {
        match self {
            Self::OnnxTrOcr(options) => json!({
                "kind": ONNX_TROCR_KIND,
                "preset": options.preset.as_str(),
                "bundleRoot": options.bundle_root,
                "cacheDir": options.cache_dir,
            }),
            Self::JsonCommand {
                name,
                command,
                args,
            } => json!({
                "kind": JSON_COMMAND_KIND,
                "name": name,
                "command": command,
                "args": args,
            }),
        }
    }
}

/// The caller's OCR configuration for one [`VisualAnalysisOcrProducer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualAnalysisOcrConfig {
    pub backend: OcrBackendConfig,
    /// Passed unchanged to `analyze_scene_aware_ocr` for every sampled frame.
    pub request: OcrRequest,
    /// Caller-declared model version (for example a model revision, bundle digest or
    /// command version). visual-analysis does not report one.
    pub model_version: Option<String>,
    pub model_fetch: ModelFetchPolicy,
}

impl VisualAnalysisOcrConfig {
    /// `backend` with `OcrRequest::default()`, no declared model version and
    /// [`ModelFetchPolicy::LocalOnly`].
    pub fn new(backend: OcrBackendConfig) -> Self {
        Self {
            backend,
            request: OcrRequest::default(),
            model_version: None,
            model_fetch: ModelFetchPolicy::LocalOnly,
        }
    }

    /// Deterministic `sha256:<64 hex>` of the effective OCR configuration (see the module
    /// documentation for what it covers).
    pub fn config_hash(&self) -> String {
        // `OcrRequest` has no serde support upstream; its `Debug` output is deterministic
        // (ordered collections only) for the pinned revision.
        let material = json!({
            "backend": self.backend.hash_material(),
            "request": format!("{:?}", self.request),
        });
        sha256_prefixed(material.to_string().as_bytes())
    }
}

/// Why a [`VisualAnalysisOcrProducer`] could not be constructed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OcrAdapterError {
    /// The backend configuration could make image-analysis-ocr download a model, and the
    /// caller did not set [`ModelFetchPolicy::AllowNetwork`]. `reason` names the
    /// download-capable option and the local alternative.
    ModelFetchNotAllowed { reason: String },
}

impl std::fmt::Display for OcrAdapterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for OcrAdapterError {}

/// [`VisibleTextProducer`] backed by visual-analysis scene-aware OCR and text tracking.
#[derive(Debug, Clone)]
pub struct VisualAnalysisOcrProducer {
    config: VisualAnalysisOcrConfig,
}

impl VisualAnalysisOcrProducer {
    /// Validates `config`. Refuses download-capable configurations unless
    /// `config.model_fetch` is [`ModelFetchPolicy::AllowNetwork`]. Does not touch the file
    /// system or the network.
    pub fn new(config: VisualAnalysisOcrConfig) -> Result<Self, OcrAdapterError> {
        if let (OcrBackendConfig::OnnxTrOcr(options), ModelFetchPolicy::LocalOnly) =
            (&config.backend, config.model_fetch)
        {
            if options.auto_download {
                return Err(OcrAdapterError::ModelFetchNotAllowed {
                    reason: "ONNX TrOCR with `auto_download` may download the model from \
                             Hugging Face; disable `auto_download` and provide the model files \
                             under `bundle_root`, or allow network access"
                        .to_string(),
                });
            }
        }
        Ok(Self { config })
    }

    pub fn config(&self) -> &VisualAnalysisOcrConfig {
        &self.config
    }
}

impl VisibleTextProducer for VisualAnalysisOcrProducer {
    fn identity(&self) -> ProducerIdentity {
        let backend = &self.config.backend;
        ProducerIdentity {
            producer: format!("{OCR_PRODUCER_PREFIX}:{}", backend.kind()),
            producer_version: VISUAL_ANALYSIS_REVISION.to_string(),
            model: backend.model_id(),
            model_version: self
                .config
                .model_version
                .clone()
                .unwrap_or_else(|| UNSPECIFIED_MODEL_VERSION.to_string()),
            config_hash: self.config.config_hash(),
        }
    }

    fn requires_scenes(&self) -> bool {
        true
    }

    fn run(&self, input: &ProducerInput<'_>) -> Result<VisibleTextRun, ProducerError> {
        if input.cancellation.is_cancelled() {
            return Err(ProducerError::Cancelled);
        }
        let failed = |reason: String| ProducerError::Failed { reason };
        let Some(scene_run) = input.scenes else {
            return Err(failed(
                "visual-analysis OCR samples the scene intervals of the same analysis, but no \
                 scene run was supplied; request scenes in the analysis profile"
                    .to_string(),
            ));
        };
        if let OcrBackendConfig::OnnxTrOcr(options) = &self.config.backend {
            if !options.bundle_root.exists() {
                return Err(failed(format!(
                    "configured ONNX TrOCR bundle_root `{}` does not exist; provide the model \
                     files locally",
                    options.bundle_root.display()
                )));
            }
        }

        let upstream = |stage: &str, error: &dyn std::fmt::Display| {
            failed(format!("visual-analysis {stage} failed: {error}"))
        };
        let mut source =
            FfmpegVideoSource::open(input.media_path).map_err(|e| upstream("video open", &e))?;
        let frame_rate = source
            .source_info()
            .video
            .as_ref()
            .and_then(|video| video.frame_rate)
            .filter(|rate| *rate.numer() > 0 && *rate.denom() > 0)
            .ok_or_else(|| failed("the video stream declares no frame rate".to_string()))?;
        let scenes = upstream_scenes(scene_run, frame_rate)?;
        let request = self.config.request.clone();
        // Content-derived inputs only, so the result depends on nothing outside the stage
        // key (content hash, producer identity, scene intervals): the track-id namespace is
        // the content hash and the duration is the end of the last scene.
        let video_id = input.media.content_hash.as_str();
        let duration = scene_run
            .scenes
            .iter()
            .map(|scene| scene.end_seconds)
            .max_by(f64::total_cmp);
        let analysis = match &self.config.backend {
            OcrBackendConfig::OnnxTrOcr(options) => {
                let backend = ocr::OnnxTrOcrBackend::from_local_model(options.clone())
                    .map_err(|e| upstream("ONNX TrOCR model load", &e))?;
                analyze_scene_aware_ocr(&mut source, backend, request, video_id, &scenes, duration)
            }
            OcrBackendConfig::JsonCommand { command, args, .. } => {
                let backend = ocr::JsonCommandOcrBackend::new(command.clone()).args(args.clone());
                analyze_scene_aware_ocr(&mut source, backend, request, video_id, &scenes, duration)
            }
        }
        .map_err(|e| upstream("scene-aware OCR", &e))?;
        drop(source);

        let identity = scene_bound_identity(&self.identity(), scene_run);
        visible_text_run_from_analysis(&identity, input.input_hash, &analysis)
    }
}

/// Maps one upstream scene-aware OCR output into a [`VisibleTextRun`] (pure; no I/O).
///
/// The receipt is [`crate::Modality::VisibleText`] with every `identity` field and
/// `input_hash`. See the module documentation for the observation and track mapping. Fails
/// with [`ProducerError::Failed`] when a text observation has neither a frame position nor
/// a timestamp, or when a track's member observations cannot be taken from the upstream
/// output.
pub fn visible_text_run_from_analysis(
    identity: &ProducerIdentity,
    input_hash: &str,
    analysis: &SceneAwareOcrAnalysis,
) -> Result<VisibleTextRun, ProducerError> {
    let failed = |reason: String| ProducerError::Failed { reason };
    let mut observations = Vec::with_capacity(analysis.observations.len());
    // Upstream observation index -> adapter observation id.
    let mut ids = vec![None; analysis.observations.len()];
    for (index, observation) in analysis.observations.iter().enumerate() {
        let Some(text) = observation.text.as_deref() else {
            continue;
        };
        let timestamp = observation
            .timestamp
            .or_else(|| observation.frame.map(|frame| frame.timestamp))
            .ok_or_else(|| {
                failed(format!(
                    "upstream OCR observation {index} has neither a frame position nor a \
                     timestamp"
                ))
            })?;
        let id = format!("obs-{index}");
        ids[index] = Some(id.clone());
        observations.push(OcrObservation {
            id,
            text: text.to_string(),
            timestamp_seconds: timestamp.seconds(),
            frame_index: observation.frame.map(|frame| frame.frame_index),
            language: observation.attributes.get("language").cloned(),
            region: observation.region.map(|region| Region {
                x: region.x,
                y: region.y,
                width: region.width,
                height: region.height,
            }),
            confidence: observation.score.map(f64::from),
        });
    }

    let mut tracks = Vec::with_capacity(analysis.text_semantics.tracks.len());
    for track in &analysis.text_semantics.tracks {
        let members: BTreeSet<usize> = track.observation_indices.iter().copied().collect();
        let observation_ids = members
            .iter()
            .map(|index| {
                ids.get(*index).cloned().flatten().ok_or_else(|| {
                    failed(format!(
                        "upstream track {} names observation {index}, which is not a text \
                         observation of this output",
                        track.id.as_str()
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let role = serde_json::to_value(track.role)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string));
        tracks.push(OcrTrack {
            id: track.id.as_str().to_string(),
            text: track.text.clone(),
            role,
            observation_ids,
        });
    }

    Ok(VisibleTextRun {
        receipt: ProducerReceipt {
            modality: Modality::VisibleText,
            producer: identity.producer.clone(),
            producer_version: identity.producer_version.clone(),
            model: identity.model.clone(),
            model_version: identity.model_version.clone(),
            config_hash: identity.config_hash.clone(),
            input_hash: input_hash.to_string(),
            run_id: None,
            config: None,
        },
        observations,
        tracks,
    })
}

/// Rebuilds the upstream scene intervals from the scene run of the same analysis. Frame
/// indices are authoritative for upstream frame sampling.
fn upstream_scenes(
    scene_run: &SceneRun,
    frame_rate: Rational64,
) -> Result<Vec<Scene>, ProducerError> {
    scene_run
        .scenes
        .iter()
        .map(|scene| match (scene.start_frame, scene.end_frame) {
            (Some(start), Some(end)) => Ok(Scene {
                start: FramePosition::from_frame_index(start, frame_rate),
                end: FramePosition::from_frame_index(end, frame_rate),
            }),
            _ => Err(ProducerError::Failed {
                reason: format!(
                    "scene `{}` has no frame range; visual-analysis OCR samples by frame index",
                    scene.id
                ),
            }),
        })
        .collect()
}
