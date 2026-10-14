//! JSON compatibility with YouTube Corpus `MediaEvidenceBatchV1`
//! (`youtube-corpus` `src/media_evidence.rs`, schema `media_evidence` version 1).
//!
//! The structs mirror the exported camelCase JSON; this crate does not depend on
//! YouTube Corpus. SponsorBlock data is not producer evidence for this core and is
//! accepted but not composed.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CompositionError, CompositionInput, MediaIdentity};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaEvidenceBatchV1 {
    pub schema: String,
    pub schema_version: u32,
    pub producer: EvidenceProducerV1,
    pub video: EvidenceVideoV1,
    pub revision: String,
    pub scenes: Vec<SceneEvidenceV1>,
    pub ocr_observations: Vec<OcrObservationEvidenceV1>,
    pub ocr_tracks: Vec<OcrTrackEvidenceV1>,
    #[serde(default)]
    pub sponsorblock: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceProducerV1 {
    pub name: String,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceVideoV1 {
    pub id: String,
    pub youtube_id: Option<String>,
    pub source_url: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessingEvidenceV1 {
    pub run_id: String,
    pub processor: String,
    pub processor_version: String,
    pub model: String,
    pub model_version: String,
    pub input_hash: String,
    pub config_hash: String,
    pub processing_config: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneEvidenceV1 {
    pub id: String,
    pub scene_index: u64,
    pub start_frame: u64,
    pub end_frame: u64,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub metadata: Value,
    pub provenance: ProcessingEvidenceV1,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrObservationEvidenceV1 {
    pub id: String,
    pub text: String,
    pub language: Option<String>,
    pub frame_index: Option<u64>,
    pub timestamp_seconds: Option<f64>,
    pub scene_index: Option<u64>,
    pub region: Option<BoundingBoxV1>,
    pub confidence: Option<f64>,
    pub attributes: Value,
    pub provenance: ProcessingEvidenceV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundingBoxV1 {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrTrackEvidenceV1 {
    pub id: String,
    pub text: String,
    pub role: String,
    pub language: Option<String>,
    pub sample_count: u32,
    pub start_frame: Option<u64>,
    pub end_frame: Option<u64>,
    pub start_seconds: Option<f64>,
    pub end_seconds: Option<f64>,
    pub region: Option<BoundingBoxV1>,
    pub metadata: Value,
    pub observation_ids: Vec<String>,
    pub scene_ids: Vec<String>,
    pub provenance: ProcessingEvidenceV1,
}

/// Converts a batch into a composition input for `media`.
///
/// Rows are grouped into producer runs by their full provenance; each
/// `ProcessingEvidenceV1` field maps onto the run's `ProducerReceipt`
/// (`processor`→`producer`, `processorVersion`→`producerVersion`, `runId`→`runId`,
/// `processingConfig`→`config`, the rest by name). The batch cannot express speech, so
/// speech is `Unprocessed`; an empty scene or OCR list cannot prove that the producer
/// ran, so it is `Unprocessed` too, never completed-empty.
pub fn composition_input_from_batch(
    media: MediaIdentity,
    batch: &MediaEvidenceBatchV1,
) -> Result<CompositionInput, CompositionError> {
    let _ = (media, batch);
    todo!("implementation pending (#25)")
}
