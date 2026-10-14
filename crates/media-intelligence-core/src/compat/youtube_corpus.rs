//! JSON compatibility with YouTube Corpus `MediaEvidenceBatchV1`
//! (`youtube-corpus` `src/media_evidence.rs`, schema `media_evidence` version 1).
//!
//! The structs mirror the exported camelCase JSON; this crate does not depend on
//! YouTube Corpus. SponsorBlock data is not producer evidence for this core and is
//! accepted but not composed.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use std::collections::BTreeMap;

use crate::{
    CompositionError, CompositionInput, MediaIdentity, Modality, ModalityInput, OcrObservation,
    OcrTrack, ProducerReceipt, Region, SceneInterval, SceneRun, VisibleTextRun,
};

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
    let mut scene_runs: BTreeMap<String, SceneRun> = BTreeMap::new();
    for scene in &batch.scenes {
        scene_runs
            .entry(run_key(&scene.provenance))
            .or_insert_with(|| SceneRun {
                receipt: receipt(Modality::Scene, &scene.provenance),
                scenes: Vec::new(),
            })
            .scenes
            .push(SceneInterval {
                id: scene.id.clone(),
                index: scene.scene_index,
                start_seconds: scene.start_seconds,
                end_seconds: scene.end_seconds,
                start_frame: Some(scene.start_frame),
                end_frame: Some(scene.end_frame),
            });
    }

    let mut ocr_runs: BTreeMap<String, VisibleTextRun> = BTreeMap::new();
    for observation in &batch.ocr_observations {
        let timestamp_seconds =
            observation
                .timestamp_seconds
                .ok_or_else(|| CompositionError::MissingTime {
                    modality: Modality::VisibleText,
                    item: observation.id.clone(),
                })?;
        ocr_run(&mut ocr_runs, &observation.provenance)
            .observations
            .push(OcrObservation {
                id: observation.id.clone(),
                text: observation.text.clone(),
                timestamp_seconds,
                frame_index: observation.frame_index,
                language: observation.language.clone(),
                region: observation.region.as_ref().map(region),
                confidence: observation.confidence,
            });
    }
    for track in &batch.ocr_tracks {
        // A track whose provenance matches no observation run keeps its own run, where
        // composition rejects its membership as unknown instead of dropping it.
        ocr_run(&mut ocr_runs, &track.provenance)
            .tracks
            .push(OcrTrack {
                id: track.id.clone(),
                text: track.text.clone(),
                role: Some(track.role.clone()),
                observation_ids: track.observation_ids.clone(),
            });
    }

    Ok(CompositionInput {
        media,
        speech: ModalityInput::Unprocessed,
        visible_text: completed_or_unprocessed(ocr_runs.into_values().collect()),
        scenes: completed_or_unprocessed(scene_runs.into_values().collect()),
    })
}

fn completed_or_unprocessed<R>(runs: Vec<R>) -> ModalityInput<R> {
    if runs.is_empty() {
        ModalityInput::Unprocessed
    } else {
        ModalityInput::Completed { runs }
    }
}

fn ocr_run<'a>(
    runs: &'a mut BTreeMap<String, VisibleTextRun>,
    provenance: &ProcessingEvidenceV1,
) -> &'a mut VisibleTextRun {
    runs.entry(run_key(provenance))
        .or_insert_with(|| VisibleTextRun {
            receipt: receipt(Modality::VisibleText, provenance),
            observations: Vec::new(),
            tracks: Vec::new(),
        })
}

fn run_key(provenance: &ProcessingEvidenceV1) -> String {
    serde_json::to_string(provenance).expect("provenance serializes")
}

fn receipt(modality: Modality, provenance: &ProcessingEvidenceV1) -> ProducerReceipt {
    ProducerReceipt {
        modality,
        producer: provenance.processor.clone(),
        producer_version: provenance.processor_version.clone(),
        model: provenance.model.clone(),
        model_version: provenance.model_version.clone(),
        config_hash: provenance.config_hash.clone(),
        input_hash: provenance.input_hash.clone(),
        run_id: Some(provenance.run_id.clone()),
        config: Some(provenance.processing_config.clone()),
    }
}

fn region(region: &BoundingBoxV1) -> Region {
    Region {
        x: region.x,
        y: region.y,
        width: region.width,
        height: region.height,
    }
}
