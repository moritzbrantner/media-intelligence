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

use std::path::PathBuf;

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

pub use crate::ModelFetchPolicy;
pub use crate::VISUAL_ANALYSIS_REVISION;
use crate::{ProducerError, ProducerIdentity, ProducerInput, VisibleTextProducer, VisibleTextRun};

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
        todo!("implementation pending (#33)")
    }

    /// The receipt `model`: the preset's Hugging Face repo id
    /// ([`OcrPreset::repo_id`]) for ONNX TrOCR, the caller's `name` for a JSON command.
    pub fn model_id(&self) -> String {
        todo!("implementation pending (#33)")
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
        let _ = backend;
        todo!("implementation pending (#33)")
    }

    /// Deterministic `sha256:<64 hex>` of the effective OCR configuration (see the module
    /// documentation for what it covers).
    pub fn config_hash(&self) -> String {
        todo!("implementation pending (#33)")
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
        let _ = config;
        todo!("implementation pending (#33)")
    }

    pub fn config(&self) -> &VisualAnalysisOcrConfig {
        &self.config
    }
}

impl VisibleTextProducer for VisualAnalysisOcrProducer {
    fn identity(&self) -> ProducerIdentity {
        todo!("implementation pending (#33)")
    }

    fn requires_scenes(&self) -> bool {
        todo!("implementation pending (#33)")
    }

    fn run(&self, input: &ProducerInput<'_>) -> Result<VisibleTextRun, ProducerError> {
        let _ = input;
        todo!("implementation pending (#33)")
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
    let _ = (identity, input_hash, analysis);
    todo!("implementation pending (#33)")
}
