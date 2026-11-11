//! JSON compatibility with YouTube Corpus `MediaEvidenceBatchV1`
//! (`youtube-corpus` `src/media_evidence.rs`, schema `media_evidence` version 1).
//!
//! The structs mirror the exported camelCase JSON; this crate does not depend on
//! YouTube Corpus. SponsorBlock data is not producer evidence for this core and is
//! accepted but not composed.

use std::collections::BTreeMap;

use media_core::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::exact_timing::{
    ExactAnchorTimingV1, ExactAnchorV1, ExactTimingError, ExactTimingSidecarV1,
};
use crate::{
    CompositionError, CompositionInput, MediaIdentity, Modality, ModalityInput, OcrObservation,
    OcrTrack, ProducerReceipt, Region, SceneInterval, SceneRun, VisibleTextRun,
};

/// Producer assertion used by the opt-in exact FFprobe decoder. A v1 seconds
/// projection alone is never proof that the original PTS was retained.
const RECORDED_FRAME_PTS: &str = "ffprobe:frame-pts";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExactSceneTimingV1 {
    schema_version: u32,
    clock: String,
    source: String,
    start: Timestamp,
    end: Timestamp,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExactOcrTimingV1 {
    schema_version: u32,
    clock: String,
    source: String,
    timestamp: Timestamp,
}

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

/// Read original PTS carried in the open metadata/attributes fields of the
/// unchanged YouTube Corpus v1 batch. No timestamp is reconstructed from
/// `startSeconds`, `endSeconds`, or `timestampSeconds`.
///
/// Returns `None` for ordinary seconds-only v1 data. If a producer asserts
/// exact timing for *any* record of a modality, every record of that modality
/// must have an anchor and the producer run must record its exact decoder.
/// Mixed/incorrect assertions fail closed instead of silently dropping data.
pub fn exact_timing_from_batch(
    media: MediaIdentity,
    batch: &MediaEvidenceBatchV1,
) -> Result<Option<ExactTimingSidecarV1>, ExactTimingError> {
    if batch.schema != "media_evidence"
        || batch.schema_version != 1
        || batch.revision.trim().is_empty()
    {
        return Err(ExactTimingError(
            "expected a versioned YouTube Corpus v1 evidence batch".into(),
        ));
    }
    let mut anchors = Vec::new();
    let mut exact_modalities = Vec::new();

    let mut exact_scenes = 0;
    for scene in &batch.scenes {
        let Some(wire) = scene.metadata.get("exactTiming") else {
            continue;
        };
        let timing: ExactSceneTimingV1 = serde_json::from_value(wire.clone()).map_err(|error| {
            ExactTimingError(format!("invalid scene {} exact PTS: {error}", scene.id))
        })?;
        validate_exact_provenance(
            timing.schema_version,
            &timing.clock,
            &timing.source,
            &scene.provenance,
            &scene.id,
        )?;
        exact_scenes += 1;
        anchors.push(ExactAnchorV1 {
            receipt: receipt(Modality::Scene, &scene.provenance),
            record_id: scene.id.clone(),
            timing: ExactAnchorTimingV1::Range {
                start: timing.start,
                end: timing.end,
            },
            frame_index: None,
            start_frame: Some(scene.start_frame),
            end_frame: Some(scene.end_frame),
        });
    }
    if exact_scenes != 0 {
        if exact_scenes != batch.scenes.len() {
            return Err(ExactTimingError(
                "scene records mix original PTS with seconds-only timing".into(),
            ));
        }
        exact_modalities.push(Modality::Scene);
    }

    let mut exact_ocr = 0;
    for observation in &batch.ocr_observations {
        let Some(wire) = observation.attributes.get("exactTiming") else {
            continue;
        };
        let timing: ExactOcrTimingV1 = serde_json::from_value(wire.clone()).map_err(|error| {
            ExactTimingError(format!("invalid OCR {} exact PTS: {error}", observation.id))
        })?;
        validate_exact_provenance(
            timing.schema_version,
            &timing.clock,
            &timing.source,
            &observation.provenance,
            &observation.id,
        )?;
        exact_ocr += 1;
        anchors.push(ExactAnchorV1 {
            receipt: receipt(Modality::VisibleText, &observation.provenance),
            record_id: observation.id.clone(),
            timing: ExactAnchorTimingV1::Instant {
                timestamp: timing.timestamp,
            },
            frame_index: observation.frame_index,
            start_frame: None,
            end_frame: None,
        });
    }
    if exact_ocr != 0 {
        if exact_ocr != batch.ocr_observations.len() {
            return Err(ExactTimingError(
                "OCR records mix original PTS with seconds-only timing".into(),
            ));
        }
        exact_modalities.push(Modality::VisibleText);
    }

    if exact_modalities.is_empty() {
        return Ok(None);
    }
    let input = composition_input_from_batch(media.clone(), batch)
        .map_err(|error| ExactTimingError(error.to_string()))?;
    let sidecar = ExactTimingSidecarV1::new(
        media,
        Some(batch.revision.clone()),
        exact_modalities,
        anchors,
    )?;
    sidecar.validate_batch_revision(&batch.revision)?;
    sidecar.validate_for(&input)?;
    Ok(Some(sidecar))
}

fn validate_exact_provenance(
    version: u32,
    clock: &str,
    source: &str,
    provenance: &ProcessingEvidenceV1,
    record: &str,
) -> Result<(), ExactTimingError> {
    if version != 1
        || clock != "media_relative"
        || source != RECORDED_FRAME_PTS
        || provenance.processing_config["exactTiming"].as_str() != Some(RECORDED_FRAME_PTS)
    {
        return Err(ExactTimingError(format!(
            "record {record} lacks recorded, aligned original FFprobe PTS provenance"
        )));
    }
    Ok(())
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
