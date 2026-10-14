//! Implementation of [`crate::compose`]: validation, evidence derivation, ordering,
//! relations and coverage.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    CompositionError, CompositionInput, CoverageStatus, EvidenceAnalysis, EvidenceContent,
    EvidenceItem, EvidenceRelation, MediaIdentity, Modality, ModalityCoverage, ModalityInput,
    OcrObservation, ProducerReceipt, RelationKind, SceneEvidence, SceneRun, SpeechEvidence,
    SpeechRun, TimedTextContract, TranscriptSegment, VisibleTextEvidence,
    VisibleTextObservationRef, VisibleTextRun, EVIDENCE_ANALYSIS_SCHEMA,
    EVIDENCE_ANALYSIS_SCHEMA_VERSION,
};

pub(crate) fn speech_run_from_timed_text(
    receipt: ProducerReceipt,
    transcript: &TimedTextContract,
) -> Result<SpeechRun, CompositionError> {
    let segments = transcript
        .segments
        .iter()
        .map(|segment| {
            let (Some(start_seconds), Some(end_seconds)) =
                (segment.start_seconds(), segment.end_seconds())
            else {
                return Err(CompositionError::UntimedSegment {
                    index: segment.index,
                });
            };
            Ok(TranscriptSegment {
                index: segment.index,
                start_seconds,
                end_seconds,
                text: segment.text.clone(),
                language: segment
                    .language
                    .clone()
                    .or_else(|| transcript.language.clone()),
                speaker: segment.speaker.clone(),
                confidence: segment.confidence().map(f64::from),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SpeechRun { receipt, segments })
}

pub(crate) fn compose(input: &CompositionInput) -> Result<EvidenceAnalysis, CompositionError> {
    let media = &input.media;
    let mut evidence = Vec::new();

    let speech = modality(&input.speech, Modality::Speech, |run: &SpeechRun| {
        (&run.receipt, speech_evidence(media, run))
    })?;
    let visible_text = modality(
        &input.visible_text,
        Modality::VisibleText,
        |run: &VisibleTextRun| (&run.receipt, visible_text_evidence(media, run)),
    )?;
    let scenes = modality(&input.scenes, Modality::Scene, |run: &SceneRun| {
        (&run.receipt, scene_evidence(media, run))
    })?;

    let mut coverage = Vec::with_capacity(3);
    for (modality, outcome) in [
        (Modality::Speech, speech),
        (Modality::VisibleText, visible_text),
        (Modality::Scene, scenes),
    ] {
        coverage.push(ModalityCoverage {
            modality,
            status: outcome.status,
        });
        evidence.extend(outcome.evidence);
    }

    evidence.sort_by(timeline_order);
    let relations = relations(&evidence);

    Ok(EvidenceAnalysis {
        schema: EVIDENCE_ANALYSIS_SCHEMA.to_string(),
        schema_version: EVIDENCE_ANALYSIS_SCHEMA_VERSION,
        media: media.clone(),
        coverage,
        evidence,
        relations,
    })
}

struct ModalityOutcome {
    status: CoverageStatus,
    evidence: Vec<EvidenceItem>,
}

fn modality<'a, R: 'a>(
    input: &'a ModalityInput<R>,
    expected: Modality,
    derive: impl Fn(
        &'a R,
    ) -> (
        &'a ProducerReceipt,
        Result<Vec<EvidenceItem>, CompositionError>,
    ),
) -> Result<ModalityOutcome, CompositionError> {
    let status = match input {
        ModalityInput::NotRequested => CoverageStatus::NotRequested,
        ModalityInput::Unprocessed => CoverageStatus::Unprocessed,
        ModalityInput::Failed { receipt, reason } => {
            if let Some(receipt) = receipt {
                check_receipt(receipt, expected)?;
            }
            CoverageStatus::Failed {
                receipt: receipt.clone(),
                reason: reason.clone(),
            }
        }
        ModalityInput::Completed { runs } => {
            if runs.is_empty() {
                return Err(CompositionError::CompletedWithoutRun { modality: expected });
            }
            let mut receipts = Vec::with_capacity(runs.len());
            let mut identities = BTreeSet::new();
            let mut evidence = Vec::new();
            for run in runs {
                let (receipt, items) = derive(run);
                check_receipt(receipt, expected)?;
                if !identities.insert(receipt_identity(receipt).to_string()) {
                    return Err(CompositionError::DuplicateRun { modality: expected });
                }
                evidence.extend(items?);
                receipts.push(receipt.clone());
            }
            receipts.sort_by_key(canonical_receipt);
            let status = if evidence.is_empty() {
                CoverageStatus::CompletedEmpty { receipts }
            } else {
                CoverageStatus::CompletedWithFindings {
                    receipts,
                    evidence_count: evidence.len(),
                }
            };
            return Ok(ModalityOutcome { status, evidence });
        }
    };
    Ok(ModalityOutcome {
        status,
        evidence: Vec::new(),
    })
}

fn check_receipt(receipt: &ProducerReceipt, expected: Modality) -> Result<(), CompositionError> {
    if receipt.modality != expected {
        return Err(CompositionError::ReceiptModalityMismatch {
            expected,
            found: receipt.modality,
        });
    }
    Ok(())
}

/// The producer identity that evidence ids are derived from. The run id and the recorded
/// configuration value are deliberately excluded: a rerun of the same producer revision and
/// configuration hash on the same input reproduces the same evidence.
fn receipt_identity(receipt: &ProducerReceipt) -> serde_json::Value {
    json!([
        receipt.modality,
        receipt.producer,
        receipt.producer_version,
        receipt.model,
        receipt.model_version,
        receipt.config_hash,
        receipt.input_hash,
    ])
}

fn canonical_receipt(receipt: &ProducerReceipt) -> String {
    serde_json::to_string(&json!([
        receipt_identity(receipt),
        receipt.run_id,
        receipt.config
    ]))
    .expect("receipt serializes")
}

fn evidence_id(
    media: &MediaIdentity,
    receipt: &ProducerReceipt,
    local_id: &serde_json::Value,
    payload: &serde_json::Value,
) -> String {
    let material = json!({
        "schema": EVIDENCE_ANALYSIS_SCHEMA,
        "schemaVersion": EVIDENCE_ANALYSIS_SCHEMA_VERSION,
        "assetId": media.asset_id.as_str(),
        "contentHash": media.content_hash.as_str(),
        "producer": receipt_identity(receipt),
        "localId": local_id,
        "payload": payload,
    });
    let bytes = serde_json::to_vec(&material).expect("evidence material serializes");
    let digest = Sha256::digest(&bytes);
    let mut id = String::with_capacity(7 + digest.len() * 2);
    id.push_str("sha256:");
    for byte in digest {
        id.push_str(&format!("{byte:02x}"));
    }
    id
}

fn check_range(
    modality: Modality,
    item: &str,
    start: f64,
    end: f64,
) -> Result<(), CompositionError> {
    if !start.is_finite() || !end.is_finite() {
        return Err(CompositionError::NonFiniteTime {
            modality,
            item: item.to_string(),
        });
    }
    if start < 0.0 || end < start {
        return Err(CompositionError::InvalidTimeRange {
            modality,
            item: item.to_string(),
        });
    }
    Ok(())
}

fn item(
    media: &MediaIdentity,
    receipt: &ProducerReceipt,
    id: String,
    start_seconds: f64,
    end_seconds: f64,
    content: EvidenceContent,
) -> EvidenceItem {
    EvidenceItem {
        id,
        modality: receipt.modality,
        start_seconds,
        end_seconds,
        asset_id: media.asset_id.clone(),
        content_hash: media.content_hash.clone(),
        receipt: receipt.clone(),
        content,
    }
}

fn speech_evidence(
    media: &MediaIdentity,
    run: &SpeechRun,
) -> Result<Vec<EvidenceItem>, CompositionError> {
    let mut seen = BTreeSet::new();
    let mut evidence = Vec::with_capacity(run.segments.len());
    for segment in &run.segments {
        let label = segment.index.to_string();
        if !seen.insert(segment.index) {
            return Err(CompositionError::DuplicateId {
                modality: Modality::Speech,
                id: label,
            });
        }
        check_range(
            Modality::Speech,
            &label,
            segment.start_seconds,
            segment.end_seconds,
        )?;
        let content = SpeechEvidence {
            segment_index: segment.index,
            text: segment.text.clone(),
            language: segment.language.clone(),
            speaker: segment.speaker.clone(),
            transcript_confidence: segment.confidence,
        };
        let id = evidence_id(
            media,
            &run.receipt,
            &json!(segment.index),
            &json!([segment.start_seconds, segment.end_seconds, &content]),
        );
        evidence.push(item(
            media,
            &run.receipt,
            id,
            segment.start_seconds,
            segment.end_seconds,
            EvidenceContent::Speech(content),
        ));
    }
    Ok(evidence)
}

fn scene_evidence(
    media: &MediaIdentity,
    run: &SceneRun,
) -> Result<Vec<EvidenceItem>, CompositionError> {
    let mut seen = BTreeSet::new();
    let mut evidence = Vec::with_capacity(run.scenes.len());
    for scene in &run.scenes {
        if !seen.insert(scene.id.as_str()) {
            return Err(CompositionError::DuplicateId {
                modality: Modality::Scene,
                id: scene.id.clone(),
            });
        }
        check_range(
            Modality::Scene,
            &scene.id,
            scene.start_seconds,
            scene.end_seconds,
        )?;
        if let (Some(start), Some(end)) = (scene.start_frame, scene.end_frame) {
            if end < start {
                return Err(CompositionError::InvalidTimeRange {
                    modality: Modality::Scene,
                    item: scene.id.clone(),
                });
            }
        }
        let content = SceneEvidence {
            scene_id: scene.id.clone(),
            scene_index: scene.index,
            start_frame: scene.start_frame,
            end_frame: scene.end_frame,
        };
        let id = evidence_id(
            media,
            &run.receipt,
            &json!(scene.id),
            &json!([scene.start_seconds, scene.end_seconds, &content]),
        );
        evidence.push(item(
            media,
            &run.receipt,
            id,
            scene.start_seconds,
            scene.end_seconds,
            EvidenceContent::Scene(content),
        ));
    }
    Ok(evidence)
}

fn observation_ref(observation: &OcrObservation) -> VisibleTextObservationRef {
    VisibleTextObservationRef {
        observation_id: observation.id.clone(),
        timestamp_seconds: observation.timestamp_seconds,
        frame_index: observation.frame_index,
        region: observation.region,
        ocr_confidence: observation.confidence,
    }
}

fn observation_order(a: &&OcrObservation, b: &&OcrObservation) -> Ordering {
    a.timestamp_seconds
        .total_cmp(&b.timestamp_seconds)
        .then_with(|| a.id.cmp(&b.id))
}

fn visible_text_evidence(
    media: &MediaIdentity,
    run: &VisibleTextRun,
) -> Result<Vec<EvidenceItem>, CompositionError> {
    let mut observations: BTreeMap<&str, &OcrObservation> = BTreeMap::new();
    for observation in &run.observations {
        if observations
            .insert(observation.id.as_str(), observation)
            .is_some()
        {
            return Err(CompositionError::DuplicateId {
                modality: Modality::VisibleText,
                id: observation.id.clone(),
            });
        }
        check_range(
            Modality::VisibleText,
            &observation.id,
            observation.timestamp_seconds,
            observation.timestamp_seconds,
        )?;
    }

    let mut track_ids = BTreeSet::new();
    let mut tracked: BTreeSet<&str> = BTreeSet::new();
    let mut evidence = Vec::with_capacity(run.tracks.len());
    for track in &run.tracks {
        if !track_ids.insert(track.id.as_str()) {
            return Err(CompositionError::DuplicateId {
                modality: Modality::VisibleText,
                id: track.id.clone(),
            });
        }
        if track.observation_ids.is_empty() {
            return Err(CompositionError::EmptyTrack {
                track_id: track.id.clone(),
            });
        }
        let mut members = Vec::with_capacity(track.observation_ids.len());
        let mut member_ids = BTreeSet::new();
        for observation_id in &track.observation_ids {
            let Some(observation) = observations.get(observation_id.as_str()) else {
                return Err(CompositionError::UnknownObservation {
                    track_id: track.id.clone(),
                    observation_id: observation_id.clone(),
                });
            };
            if !member_ids.insert(observation_id.as_str()) {
                // Listing the same frame twice in one track is harmless; count it once.
                continue;
            }
            if !tracked.insert(observation.id.as_str()) {
                return Err(CompositionError::ObservationInMultipleTracks {
                    observation_id: observation_id.clone(),
                });
            }
            members.push(*observation);
        }
        members.sort_by(observation_order);
        let start = members[0].timestamp_seconds;
        let end = members[members.len() - 1].timestamp_seconds;
        let content = VisibleTextEvidence {
            track_id: Some(track.id.clone()),
            text: track.text.clone(),
            role: track.role.clone(),
            observations: members.iter().map(|o| observation_ref(o)).collect(),
        };
        let id = evidence_id(
            media,
            &run.receipt,
            &json!({ "track": track.id }),
            &json!([start, end, &content]),
        );
        evidence.push(item(
            media,
            &run.receipt,
            id,
            start,
            end,
            EvidenceContent::VisibleText(content),
        ));
    }

    let mut untracked: Vec<&OcrObservation> = observations
        .values()
        .copied()
        .filter(|observation| !tracked.contains(observation.id.as_str()))
        .collect();
    untracked.sort_by(observation_order);
    for observation in untracked {
        let time = observation.timestamp_seconds;
        let content = VisibleTextEvidence {
            track_id: None,
            text: observation.text.clone(),
            role: None,
            observations: vec![observation_ref(observation)],
        };
        let id = evidence_id(
            media,
            &run.receipt,
            &json!({ "observation": observation.id }),
            &json!([time, time, &content, observation.language]),
        );
        evidence.push(item(
            media,
            &run.receipt,
            id,
            time,
            time,
            EvidenceContent::VisibleText(content),
        ));
    }
    Ok(evidence)
}

fn timeline_order(a: &EvidenceItem, b: &EvidenceItem) -> Ordering {
    a.start_seconds
        .total_cmp(&b.start_seconds)
        .then_with(|| a.end_seconds.total_cmp(&b.end_seconds))
        .then_with(|| a.modality.cmp(&b.modality))
        .then_with(|| a.id.cmp(&b.id))
}

/// Half-open overlap; an instantaneous item is the point `t`, which lies in `[a, b)` iff
/// `a <= t < b`, and two points overlap iff they are equal.
fn overlaps(a: &EvidenceItem, b: &EvidenceItem) -> bool {
    let a_point = a.start_seconds == a.end_seconds;
    let b_point = b.start_seconds == b.end_seconds;
    match (a_point, b_point) {
        (true, true) => a.start_seconds == b.start_seconds,
        (true, false) => b.start_seconds <= a.start_seconds && a.start_seconds < b.end_seconds,
        (false, true) => a.start_seconds <= b.start_seconds && b.start_seconds < a.end_seconds,
        (false, false) => a.start_seconds < b.end_seconds && b.start_seconds < a.end_seconds,
    }
}

fn relations(evidence: &[EvidenceItem]) -> Vec<EvidenceRelation> {
    let of = |modality: Modality| -> Vec<&EvidenceItem> {
        evidence.iter().filter(|e| e.modality == modality).collect()
    };
    let speech = of(Modality::Speech);
    let visible_text = of(Modality::VisibleText);
    let scenes = of(Modality::Scene);

    let mut relations = BTreeSet::new();
    let mut relate = |kind, subjects: &[&EvidenceItem], objects: &[&EvidenceItem]| {
        for subject in subjects {
            for object in objects {
                if overlaps(subject, object) {
                    relations.insert(EvidenceRelation {
                        kind,
                        subject: subject.id.clone(),
                        object: object.id.clone(),
                    });
                }
            }
        }
    };
    relate(RelationKind::SceneSpeech, &scenes, &speech);
    relate(RelationKind::SceneVisibleText, &scenes, &visible_text);
    relate(
        RelationKind::SpeechVisibleTextCooccurrence,
        &speech,
        &visible_text,
    );
    relations.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AssetId, ContentHash, OcrTrack};

    fn receipt(modality: Modality) -> ProducerReceipt {
        ProducerReceipt {
            modality,
            producer: "visual-analysis".to_string(),
            producer_version: "1".to_string(),
            model: "ocr".to_string(),
            model_version: "1".to_string(),
            config_hash: "c".to_string(),
            input_hash: "i".to_string(),
            run_id: None,
            config: None,
        }
    }

    fn observation(id: &str, time: f64) -> OcrObservation {
        OcrObservation {
            id: id.to_string(),
            text: "slide".to_string(),
            timestamp_seconds: time,
            frame_index: None,
            language: None,
            region: None,
            confidence: None,
        }
    }

    fn input(runs: Vec<VisibleTextRun>) -> CompositionInput {
        CompositionInput {
            media: MediaIdentity {
                asset_id: AssetId::new("asset").unwrap(),
                content_hash: ContentHash::from_bytes(b"bytes"),
                duration_seconds: None,
            },
            speech: ModalityInput::NotRequested,
            visible_text: ModalityInput::Completed { runs },
            scenes: ModalityInput::NotRequested,
        }
    }

    fn track(id: &str, members: &[&str]) -> OcrTrack {
        OcrTrack {
            id: id.to_string(),
            text: "slide".to_string(),
            role: None,
            observation_ids: members.iter().map(|m| m.to_string()).collect(),
        }
    }

    fn run(tracks: Vec<OcrTrack>) -> VisibleTextRun {
        VisibleTextRun {
            receipt: receipt(Modality::VisibleText),
            observations: vec![observation("a", 1.0), observation("b", 2.0)],
            tracks,
        }
    }

    #[test]
    fn empty_tracks_are_rejected() {
        assert_eq!(
            compose(&input(vec![run(vec![track("t", &[])])])).unwrap_err(),
            CompositionError::EmptyTrack {
                track_id: "t".to_string()
            }
        );
    }

    #[test]
    fn an_observation_cannot_count_in_two_tracks() {
        let tracks = vec![track("t1", &["a", "b"]), track("t2", &["b"])];
        assert_eq!(
            compose(&input(vec![run(tracks)])).unwrap_err(),
            CompositionError::ObservationInMultipleTracks {
                observation_id: "b".to_string()
            }
        );
    }

    #[test]
    fn a_member_listed_twice_counts_once() {
        let analysis = compose(&input(vec![run(vec![track("t", &["a", "b", "a"])])])).unwrap();
        assert_eq!(analysis.evidence.len(), 1);
        let EvidenceContent::VisibleText(text) = &analysis.evidence[0].content else {
            panic!()
        };
        assert_eq!(text.observations.len(), 2);
    }

    #[test]
    fn runs_with_the_same_producer_identity_are_rejected() {
        let mut second = run(Vec::new());
        second.receipt.run_id = Some("another run".to_string());
        assert_eq!(
            compose(&input(vec![run(Vec::new()), second])).unwrap_err(),
            CompositionError::DuplicateRun {
                modality: Modality::VisibleText
            }
        );
    }

    #[test]
    fn rerun_ids_ignore_run_identity_and_recorded_config() {
        let first = compose(&input(vec![run(Vec::new())])).unwrap();
        let mut rerun = run(Vec::new());
        rerun.receipt.run_id = Some("rerun".to_string());
        rerun.receipt.config = Some(json!({ "recorded": true }));
        let second = compose(&input(vec![rerun])).unwrap();
        let ids = |analysis: &EvidenceAnalysis| {
            analysis
                .evidence
                .iter()
                .map(|e| e.id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&first), ids(&second));
    }
}
