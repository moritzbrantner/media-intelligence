//! Embeddable composition of already-recorded producer evidence for one caller-owned
//! media object into one deterministic, source-grounded multimodal analysis.
//!
//! The crate composes; it does not produce. Speech recognition, OCR (including OCR text
//! tracking, owned by `visual-analysis` `analyze_video_text_semantics`) and scene
//! detection stay with their canonical producers. Callers hand in recorded producer
//! runs with their receipts; [`compose`] aligns them on one source-relative timeline,
//! derives time-overlap relations and reports per-modality coverage.
//!
//! # Contract fixed by the acceptance tests (#25)
//!
//! * Times are seconds relative to the start of the media object. Every evidence item
//!   occupies the half-open interval `[start_seconds, end_seconds)`. An instantaneous
//!   item (`start_seconds == end_seconds`, for example a single OCR frame) is the point
//!   `t`; a point overlaps an interval `[a, b)` iff `a <= t < b`, and two points overlap
//!   iff they are equal. Touching intervals (`end == other.start`) do not overlap.
//! * Evidence is ordered by `start_seconds`, then `end_seconds`, then [`Modality`]
//!   (declaration order), then evidence id (byte-wise string order).
//! * Producer-local ids (segment indices, scene ids, observation ids, track ids) are
//!   scoped to their producer run; runs are never merged, even when two runs of the same
//!   modality report identical content.
//! * Repeated OCR frames collapse only through producer-supplied track membership. A
//!   track spans from its earliest to its latest member observation timestamp; an
//!   observation that belongs to no track is its own instantaneous item.
//! * Confidence values keep their producer- and modality-specific meaning; there is no
//!   shared score field.
//!
//! Status: the public surface below is the acceptance seam for issue #25; the
//! behaviour is not implemented yet.

pub mod compat;

use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

pub use corpus_core::{AssetId, ContentHash};
pub use media_core::TimedTextContract;

/// Schema name written into every [`EvidenceAnalysis`].
pub const EVIDENCE_ANALYSIS_SCHEMA: &str = "media-intelligence.evidence-analysis";
/// Schema version written into every [`EvidenceAnalysis`].
pub const EVIDENCE_ANALYSIS_SCHEMA_VERSION: u32 = 1;

/// Caller-owned identity of the one media object being analysed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaIdentity {
    /// Corpus-owned asset identity (Foundation `corpus-core`).
    pub asset_id: AssetId,
    /// Exact content revision of the analysed bytes.
    pub content_hash: ContentHash,
    /// Known duration, when the caller has it.
    #[serde(default)]
    pub duration_seconds: Option<f64>,
}

/// Evidence modality. Declaration order is the timeline tie-break order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    /// Spoken transcript segments.
    Speech,
    /// OCR visible text (tracks or untracked observations).
    VisibleText,
    /// Scene intervals.
    Scene,
}

/// Identity of the producer run that recorded a piece of evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProducerReceipt {
    /// Modality this run produced.
    pub modality: Modality,
    /// Producer (processor) name, e.g. `visual-analysis`.
    pub producer: String,
    /// Exact producer version or revision.
    pub producer_version: String,
    /// Model or algorithm name.
    pub model: String,
    /// Exact model version.
    pub model_version: String,
    /// Hash of the effective producer configuration.
    pub config_hash: String,
    /// Hash of the producer's input bytes.
    pub input_hash: String,
    /// Producer-side run identity, when one exists.
    #[serde(default)]
    pub run_id: Option<String>,
    /// Producer-recorded configuration, carried through untouched.
    #[serde(default)]
    pub config: Option<serde_json::Value>,
}

/// One transcript segment as recorded by a speech producer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    /// Producer-local segment index (unique within its run).
    pub index: u64,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub text: String,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub speaker: Option<String>,
    /// Transcription confidence exactly as the speech producer reported it.
    #[serde(default)]
    pub confidence: Option<f64>,
}

/// One recorded speech producer run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechRun {
    pub receipt: ProducerReceipt,
    pub segments: Vec<TranscriptSegment>,
}

impl SpeechRun {
    /// Adopts a Foundation timed-text transcript (the shape `audio-analysis` emits through
    /// `audio-timed-text-compat`). Segments without a time range cannot be placed on the
    /// timeline and are rejected.
    pub fn from_timed_text(
        receipt: ProducerReceipt,
        transcript: &TimedTextContract,
    ) -> Result<Self, CompositionError> {
        let _ = (receipt, transcript);
        todo!("implementation pending (#25)")
    }
}

/// One scene interval as recorded by a scene producer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneInterval {
    /// Producer-local scene id (unique within its run).
    pub id: String,
    pub index: u64,
    pub start_seconds: f64,
    pub end_seconds: f64,
    #[serde(default)]
    pub start_frame: Option<u64>,
    #[serde(default)]
    pub end_frame: Option<u64>,
}

/// One recorded scene producer run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneRun {
    pub receipt: ProducerReceipt,
    pub scenes: Vec<SceneInterval>,
}

/// Pixel region of an OCR observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// One frame-level OCR observation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrObservation {
    /// Producer-local observation id (unique within its run).
    pub id: String,
    pub text: String,
    /// Source-relative time of the sampled frame.
    pub timestamp_seconds: f64,
    #[serde(default)]
    pub frame_index: Option<u64>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub region: Option<Region>,
    /// OCR confidence exactly as the OCR producer reported it.
    #[serde(default)]
    pub confidence: Option<f64>,
}

/// Producer-supplied grouping of repeated OCR observations into one text occurrence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrTrack {
    /// Producer-local track id (unique within its run).
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub role: Option<String>,
    /// Member observations of the same run.
    pub observation_ids: Vec<String>,
}

/// One recorded OCR producer run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisibleTextRun {
    pub receipt: ProducerReceipt,
    pub observations: Vec<OcrObservation>,
    #[serde(default)]
    pub tracks: Vec<OcrTrack>,
}

/// What the caller knows about one modality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ModalityInput<R> {
    /// The caller did not ask for this modality.
    NotRequested,
    /// Requested, but no producer result exists (yet).
    Unprocessed,
    /// The producer ran and failed.
    Failed {
        #[serde(default)]
        receipt: Option<ProducerReceipt>,
        reason: String,
    },
    /// One or more completed producer runs. A completed modality must name at least one run.
    Completed { runs: Vec<R> },
}

/// Everything [`compose`] needs for one media object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositionInput {
    pub media: MediaIdentity,
    pub speech: ModalityInput<SpeechRun>,
    pub visible_text: ModalityInput<VisibleTextRun>,
    pub scenes: ModalityInput<SceneRun>,
}

/// Composed, source-grounded analysis of one media object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceAnalysis {
    pub schema: String,
    pub schema_version: u32,
    pub media: MediaIdentity,
    /// Exactly one entry per [`Modality`], in declaration order.
    pub coverage: Vec<ModalityCoverage>,
    /// Timeline-ordered evidence.
    pub evidence: Vec<EvidenceItem>,
    /// Sorted, duplicate-free time-overlap relations.
    pub relations: Vec<EvidenceRelation>,
}

/// Coverage of one modality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModalityCoverage {
    pub modality: Modality,
    pub status: CoverageStatus,
}

/// Distinguishable coverage outcomes; a missing modality is never `CompletedEmpty`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CoverageStatus {
    NotRequested,
    Unprocessed,
    Failed {
        receipt: Option<ProducerReceipt>,
        reason: String,
    },
    /// Completed runs that yielded no evidence.
    CompletedEmpty {
        receipts: Vec<ProducerReceipt>,
    },
    /// Completed runs that yielded `evidence_count` evidence items.
    #[serde(rename_all = "camelCase")]
    CompletedWithFindings {
        receipts: Vec<ProducerReceipt>,
        evidence_count: usize,
    },
}

/// One composed evidence item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceItem {
    /// Deterministic content-addressed id.
    pub id: String,
    pub modality: Modality,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub asset_id: AssetId,
    pub content_hash: ContentHash,
    pub receipt: ProducerReceipt,
    pub content: EvidenceContent,
}

/// Modality-specific payload; scores live here with their modality-specific meaning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvidenceContent {
    Speech(SpeechEvidence),
    VisibleText(VisibleTextEvidence),
    Scene(SceneEvidence),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechEvidence {
    pub segment_index: u64,
    pub text: String,
    pub language: Option<String>,
    pub speaker: Option<String>,
    pub transcript_confidence: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisibleTextEvidence {
    /// Producer track id; `None` for an untracked observation.
    pub track_id: Option<String>,
    pub text: String,
    pub role: Option<String>,
    /// Every contributing observation, ordered by timestamp then id.
    pub observations: Vec<VisibleTextObservationRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisibleTextObservationRef {
    pub observation_id: String,
    pub timestamp_seconds: f64,
    pub frame_index: Option<u64>,
    pub region: Option<Region>,
    pub ocr_confidence: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneEvidence {
    pub scene_id: String,
    pub scene_index: u64,
    pub start_frame: Option<u64>,
    pub end_frame: Option<u64>,
}

/// Time-overlap relation between two evidence items.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRelation {
    pub kind: RelationKind,
    /// Scene item for scene relations; speech item for co-occurrence.
    pub subject: String,
    /// Speech or visible-text item.
    pub object: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    /// Scene overlaps a speech segment.
    SceneSpeech,
    /// Scene overlaps visible text.
    SceneVisibleText,
    /// Speech segment overlaps visible text.
    SpeechVisibleTextCooccurrence,
}

/// Why an input could not be composed.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum CompositionError {
    /// A time is NaN or infinite.
    NonFiniteTime { modality: Modality, item: String },
    /// `end < start`, or a time before the start of the media.
    InvalidTimeRange { modality: Modality, item: String },
    /// A timed-text segment carries no time range.
    UntimedSegment { index: u64 },
    /// A track names an observation that its run does not contain.
    UnknownObservation {
        track_id: String,
        observation_id: String,
    },
    /// A run's receipt names a different modality than the slot it was supplied in.
    ReceiptModalityMismatch { expected: Modality, found: Modality },
    /// A modality is `Completed` without any producer run.
    CompletedWithoutRun { modality: Modality },
    /// A producer-local id occurs twice within one run.
    DuplicateId { modality: Modality, id: String },
}

impl Display for CompositionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for CompositionError {}

/// Composes one media object's recorded evidence into an [`EvidenceAnalysis`].
pub fn compose(input: &CompositionInput) -> Result<EvidenceAnalysis, CompositionError> {
    let _ = input;
    todo!("implementation pending (#25)")
}
