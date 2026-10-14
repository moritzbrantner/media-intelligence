//! One bounded native media-analysis operation for one caller-owned, locally available
//! media file (#27, slice 1).
//!
//! [`analyze`] selects the stages the caller's [`AnalysisProfile`] requests, runs them,
//! and composes their runs through [`media_intelligence_core::compose`] into one
//! [`EvidenceAnalysis`]. It reports one [`StageStatus`] per modality.
//!
//! * **Scenes** run for real through the canonical owner path: `visual-analysis`
//!   `video-analysis-ffmpeg::FfmpegVideoSource` plus
//!   `video-analysis-ingest::surface::detect_content_scenes`, pinned to
//!   [`VISUAL_ANALYSIS_REVISION`]. Scene detection is scenedetect-core inside
//!   visual-analysis; this crate contains no detector code.
//! * **Speech** and **visible text** are typed producer seams ([`SpeechProducer`],
//!   [`VisibleTextProducer`]) that the caller (or a later adapter) implements. A requested
//!   modality without a producer is [`StageStatus::Unavailable`]; it is never reported as
//!   completed.
//! * With the opt-in `audio-analysis` feature, the `speech` module provides the canonical
//!   speech producer adapter over `audio-analysis` transcription (#32).
//! * With the opt-in `visual-analysis-ocr` feature, the `visible_text` module provides the
//!   canonical visible-text producer adapter over visual-analysis scene-aware OCR and text
//!   tracking (#33).
//!
//! # Contract fixed by the acceptance tests (#27 slice 1)
//!
//! * The caller owns media identity and storage. Before any stage runs, the bytes at
//!   [`AnalysisRequest::media_path`] must exist and hash (SHA-256) to the declared
//!   [`MediaIdentity::content_hash`]; otherwise [`analyze`] returns an error and runs no
//!   producer. A stage whose input bytes change while the operation runs must not be
//!   reported as completed for the declared content hash.
//! * Every receipt's `input_hash` is `sha256:<content hash hex>` ([`input_hash`]).
//! * Stages run in the order speech, scenes, visible text (#33; #27 slice 1 used the
//!   [`Modality`] declaration order speech, visible text, scenes). The visible-text producer
//!   receives the scene run completed or reused by the same analysis in
//!   [`ProducerInput::scenes`]; every other producer receives `None`.
//! * A visible-text producer whose [`VisibleTextProducer::requires_scenes`] is `true`
//!   (#33) depends on that scene run: without one (scenes not requested, failed, or the
//!   run rejected) its stage is [`StageStatus::Failed`] with a reason that names the scene
//!   stage, and the producer is not invoked; this crate never runs a second, hidden scene
//!   detection. Such a producer is keyed in the [`StageCache`] and receipted under
//!   [`scene_bound_identity`], so a different scene run never reuses its cached result.
//! * A stage is executed only when it is requested. Not-requested stages are never
//!   executed, even when a producer is supplied.
//! * Stage results are reused through a caller-owned [`StageCache`] keyed by
//!   [`StageKey`]: source content hash, modality, and producer identity including the
//!   configuration hash. A cache hit is [`StageStatus::Reused`] and the producer is not
//!   invoked. Only successfully completed runs are stored; failures are not cached.
//! * Cancellation ([`CancellationToken`]) stops new work. A stage that has not started
//!   when the token is cancelled is [`StageStatus::Cancelled`] and contributes no
//!   evidence. A producer that observes cancellation reports [`ProducerError::Cancelled`],
//!   which is also [`StageStatus::Cancelled`].
//! * A failed stage does not discard independently successful stages.
//! * In the composed analysis, unavailable and cancelled stages are core
//!   `Unprocessed`, failed stages are core `Failed`, and completed or reused stages are
//!   core `Completed` with their run.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

mod execution;
#[cfg(feature = "audio-analysis")]
pub mod speech;
#[cfg(feature = "visual-analysis-ocr")]
pub mod visible_text;

pub use media_intelligence_core::{
    AssetId, CompositionError, ContentHash, EvidenceAnalysis, MediaIdentity, Modality,
    ProducerReceipt, SceneRun, SpeechRun, VisibleTextRun,
};

/// Exact `visual-analysis` revision whose scene path this crate invokes.
pub const VISUAL_ANALYSIS_REVISION: &str = "115c1f0caceb644993df3121b4625e674a9f1b7a";
/// Producer name written into scene receipts.
pub const SCENE_PRODUCER: &str = "visual-analysis";
/// Model (algorithm) name written into scene receipts.
pub const SCENE_MODEL: &str = "scenedetect-core:content";
/// Model version written into scene receipts (the scenedetect-core crate version in the graph).
pub const SCENE_MODEL_VERSION: &str = "0.1.0";

/// Content-scene detection configuration, passed to
/// `video_analysis_ingest::surface::detect_content_scenes`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneConfig {
    /// Content detector threshold; finite and non-negative.
    pub threshold: f32,
    /// Minimum scene length in frames; greater than zero.
    pub min_scene_len: u64,
}

impl Default for SceneConfig {
    /// Threshold 27.0 and minimum scene length 15 frames (the YouTube Corpus defaults).
    fn default() -> Self {
        Self {
            threshold: 27.0,
            min_scene_len: 15,
        }
    }
}

impl SceneConfig {
    /// Deterministic `sha256:<hex>` hash of the effective configuration. Equal
    /// configurations give equal hashes; changing any field changes the hash.
    pub fn config_hash(&self) -> String {
        let identity = format!(
            "visual-analysis.detect_content_scenes;threshold-bits={:08x};min-scene-len={}",
            self.threshold.to_bits(),
            self.min_scene_len
        );
        execution::sha256_prefixed(identity.as_bytes())
    }

    fn validate(&self) -> Result<(), AnalysisError> {
        if !self.threshold.is_finite() || self.threshold < 0.0 {
            return Err(AnalysisError::InvalidSceneConfig {
                reason: format!(
                    "scene threshold must be finite and non-negative, got {}",
                    self.threshold
                ),
            });
        }
        if self.min_scene_len == 0 {
            return Err(AnalysisError::InvalidSceneConfig {
                reason: "minimum scene length must be greater than zero".to_string(),
            });
        }
        Ok(())
    }

    fn producer_identity(&self) -> ProducerIdentity {
        ProducerIdentity {
            producer: SCENE_PRODUCER.to_string(),
            producer_version: VISUAL_ANALYSIS_REVISION.to_string(),
            model: SCENE_MODEL.to_string(),
            model_version: SCENE_MODEL_VERSION.to_string(),
            config_hash: self.config_hash(),
        }
    }
}

/// Which modalities to analyse. A modality not selected here is never executed.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisProfile {
    /// Run the caller-supplied speech producer.
    pub speech: bool,
    /// Run the caller-supplied visible-text producer.
    pub visible_text: bool,
    /// Run canonical scene detection with this configuration.
    pub scenes: Option<SceneConfig>,
}

/// One analysis of one caller-owned local media file.
#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisRequest {
    /// Caller-owned identity; `content_hash` must describe the bytes at `media_path`.
    pub media: MediaIdentity,
    /// Already available local media file. Nothing is downloaded.
    pub media_path: PathBuf,
    pub profile: AnalysisProfile,
}

/// Identity of a producer as it would appear in its receipt. Changing any field (for
/// example `config_hash` after a provider configuration change) invalidates cached runs
/// of that producer only.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProducerIdentity {
    pub producer: String,
    pub producer_version: String,
    pub model: String,
    pub model_version: String,
    pub config_hash: String,
}

/// What a producer receives for one run.
#[derive(Debug, Clone, Copy)]
pub struct ProducerInput<'a> {
    pub media: &'a MediaIdentity,
    pub media_path: &'a Path,
    /// The value the run's receipt must carry as `input_hash` (see [`input_hash`]).
    pub input_hash: &'a str,
    /// Long-running producers poll this and return [`ProducerError::Cancelled`].
    pub cancellation: &'a CancellationToken,
    /// The scene run completed or reused by the same analysis (#33). Only the visible-text
    /// stage, which runs after scenes, receives it; it is `None` for speech and scenes, and
    /// for visible text when scenes were not requested or did not complete.
    pub scenes: Option<&'a SceneRun>,
}

/// Why a producer did not return a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProducerError {
    /// The producer ran and failed (missing model, decode error, ...).
    Failed { reason: String },
    /// The producer stopped because the operation was cancelled.
    Cancelled,
}

/// Caller-implemented speech producer (for example a later `audio-analysis` adapter).
///
/// The returned run's receipt must name [`Modality::Speech`], this producer's
/// [`identity`](Self::identity) and the input's `input_hash`; otherwise the stage is
/// [`StageStatus::Failed`] and its run is not composed.
pub trait SpeechProducer {
    fn identity(&self) -> ProducerIdentity;
    fn run(&self, input: &ProducerInput<'_>) -> Result<SpeechRun, ProducerError>;
}

/// Caller-implemented visible-text (OCR) producer; same receipt rules as
/// [`SpeechProducer`], with [`Modality::VisibleText`].
///
/// A producer that samples frames from the scene intervals of the same analysis (#33)
/// returns `true` from [`requires_scenes`](Self::requires_scenes). Its run's receipt must
/// then name [`scene_bound_identity`] of its identity and the scene run it received.
pub trait VisibleTextProducer {
    fn identity(&self) -> ProducerIdentity;

    /// Whether this producer needs [`ProducerInput::scenes`] (#33). Defaults to `false`;
    /// such producers are keyed, receipted and run exactly as in #27 slice 1.
    fn requires_scenes(&self) -> bool {
        false
    }

    fn run(&self, input: &ProducerInput<'_>) -> Result<VisibleTextRun, ProducerError>;
}

/// The identity under which a scene-dependent visible-text producer's run is cached and
/// receipted (#33): `identity` with `config_hash` replaced by a deterministic
/// `sha256:<64 hex>` over `identity.config_hash` and the scene run's receipt identity
/// (producer, producer version, model, model version and config hash). Every other field is
/// unchanged. Equal inputs give equal results; a different scene configuration (a different
/// scene receipt `config_hash`) or a different own `config_hash` gives a different hash.
pub fn scene_bound_identity(identity: &ProducerIdentity, scenes: &SceneRun) -> ProducerIdentity {
    let _ = (identity, scenes);
    todo!("implementation pending (#33)")
}

/// Whether a producer adapter may fetch models over the network (#32, #33).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelFetchPolicy {
    /// Local models only (the default). Download-capable configurations are refused.
    #[default]
    LocalOnly,
    /// The caller explicitly allows first-run model downloads and network access.
    AllowNetwork,
}

/// The caller-supplied producers. `None` means the modality is unavailable.
#[derive(Clone, Copy, Default)]
pub struct Producers<'a> {
    pub speech: Option<&'a dyn SpeechProducer>,
    pub visible_text: Option<&'a dyn VisibleTextProducer>,
}

/// Cache key of one stage result.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageKey {
    pub modality: Modality,
    pub content_hash: ContentHash,
    pub producer: ProducerIdentity,
}

/// One cached, successfully completed stage run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "modality", rename_all = "snake_case")]
pub enum CachedStage {
    Speech(SpeechRun),
    VisibleText(VisibleTextRun),
    Scenes(SceneRun),
}

/// Caller-owned storage of stage results. The caller decides where and how long results
/// live; this crate only reads and writes through this trait.
pub trait StageCache {
    fn get(&self, key: &StageKey) -> Option<CachedStage>;
    fn put(&mut self, key: StageKey, stage: CachedStage);
}

/// Simple process-local [`StageCache`].
#[derive(Debug, Clone, Default)]
pub struct InMemoryStageCache {
    entries: Vec<(StageKey, CachedStage)>,
}

impl InMemoryStageCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of stored stage results.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Stored keys, in insertion order.
    pub fn keys(&self) -> Vec<StageKey> {
        self.entries.iter().map(|(key, _)| key.clone()).collect()
    }
}

impl StageCache for InMemoryStageCache {
    fn get(&self, key: &StageKey) -> Option<CachedStage> {
        self.entries
            .iter()
            .find(|(stored, _)| stored == key)
            .map(|(_, stage)| stage.clone())
    }

    fn put(&mut self, key: StageKey, stage: CachedStage) {
        match self.entries.iter_mut().find(|(stored, _)| *stored == key) {
            Some(entry) => entry.1 = stage,
            None => self.entries.push((key, stage)),
        }
    }
}

/// Cloneable cancellation handle shared between the caller and the running operation.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// Execution status of one modality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum StageStatus {
    /// The profile did not select this modality; nothing was executed.
    NotRequested,
    /// Requested, but no producer is available.
    Unavailable,
    /// Requested, but cancellation stopped it before it produced a run.
    Cancelled,
    /// A cached run for the same content hash and producer identity was reused; the
    /// producer was not invoked.
    #[serde(rename_all = "camelCase")]
    Reused {
        receipt: ProducerReceipt,
        evidence_count: usize,
    },
    /// The producer ran in this operation and yielded `evidence_count > 0` items.
    #[serde(rename_all = "camelCase")]
    CompletedWithFindings {
        receipt: ProducerReceipt,
        evidence_count: usize,
    },
    /// The producer ran in this operation and yielded no evidence.
    CompletedEmpty { receipt: ProducerReceipt },
    /// The producer ran and failed, or its run was rejected.
    Failed {
        receipt: Option<ProducerReceipt>,
        reason: String,
    },
}

/// Result of [`analyze`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisReport {
    pub speech: StageStatus,
    pub visible_text: StageStatus,
    pub scenes: StageStatus,
    /// All stage runs composed by `media_intelligence_core::compose`.
    pub analysis: EvidenceAnalysis,
}

impl AnalysisReport {
    /// The status of `modality`.
    pub fn status(&self, modality: Modality) -> &StageStatus {
        match modality {
            Modality::Speech => &self.speech,
            Modality::VisibleText => &self.visible_text,
            Modality::Scene => &self.scenes,
        }
    }
}

/// Why [`analyze`] could not produce a report at all.
#[derive(Debug)]
#[non_exhaustive]
pub enum AnalysisError {
    /// The media file does not exist or is not a regular file.
    MediaMissing { path: PathBuf },
    /// The media file could not be read.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The bytes at the media path do not hash to the declared content hash.
    ContentHashMismatch {
        declared: ContentHash,
        actual: ContentHash,
    },
    /// The requested scene configuration is invalid (non-finite or negative threshold,
    /// zero minimum scene length).
    InvalidSceneConfig { reason: String },
    /// The stage runs could not be composed.
    Composition(CompositionError),
}

impl std::fmt::Display for AnalysisError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for AnalysisError {}

/// The receipt `input_hash` for media with this content hash: `sha256:<hex>`.
pub fn input_hash(content_hash: &ContentHash) -> String {
    format!("sha256:{}", content_hash.as_str())
}

/// Runs the requested stages for one caller-owned media file and composes their runs.
pub fn analyze(
    request: &AnalysisRequest,
    producers: &Producers<'_>,
    cache: &mut dyn StageCache,
    cancellation: &CancellationToken,
) -> Result<AnalysisReport, AnalysisError> {
    execution::analyze(request, producers, cache, cancellation)
}
