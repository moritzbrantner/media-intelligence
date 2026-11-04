//! Portable synthetic VFR fixture: Foundation exact PTS, YouTube Corpus v1,
//! scene/OCR adapters, and Media Intelligence v1 composition.
//!
//! No decoder is exercised here. This is the representation and integration
//! boundary test; a real-media replay is a separate acceptance requirement.

mod common;

use std::cmp::Ordering;
use std::collections::BTreeSet;

use common::{coverage, fixture_text, item, labelled_relations, serialized};
use media_core::{MediaRange, Timebase, Timestamp};
use media_intelligence_core::compat::youtube_corpus::{
    composition_input_from_batch, exact_timing_from_batch, MediaEvidenceBatchV1,
};
use media_intelligence_core::exact_timing::{
    compose_with_exact_timing, ExactAnchorTimingV1, ExactAnchorV1, ExactEvidenceAnalysisV1,
    ExactTimingSidecarV1,
};
use media_intelligence_core::{
    compose, AssetId, ContentHash, CoverageStatus, MediaIdentity, Modality, ModalityInput,
    RelationKind, SpeechRun,
};
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(&fixture_text("vfr_interoperability_v1.json")).unwrap()
}

fn timestamp(value: &Value) -> Timestamp {
    // Foundation is the canonical authority for rational timestamp validation,
    // integer-exact ordering, and the decimal-string PTS wire contract.
    serde_json::from_value(value.clone()).expect("valid Foundation timestamp")
}

#[test]
fn vfr_pts_and_cut_remain_exact_across_timebases() {
    let fixture = fixture();
    let frames = fixture["frameTimestamps"].as_array().unwrap();
    assert_eq!(frames.len(), 7);
    let stamps: Vec<_> = frames
        .iter()
        .enumerate()
        .map(|(index, frame)| {
            assert_eq!(frame["index"].as_u64(), Some(index as u64));
            let original = &frame["timestamp"];
            let stamp = timestamp(original);
            assert_eq!(&serde_json::to_value(stamp).unwrap(), original);
            stamp
        })
        .collect();
    for pair in stamps.windows(2) {
        assert_eq!(pair[0].chronological_cmp(pair[1]).unwrap(), Ordering::Less);
    }
    let differences: BTreeSet<i64> = stamps.windows(2).map(|w| w[1].pts - w[0].pts).collect();
    assert!(differences.len() >= 3, "this must be variable frame rate");

    let cut = timestamp(&fixture["cut"]);
    let duration = timestamp(&fixture["syntheticSource"]["duration"]);
    let first = MediaRange::new(stamps[0], cut).unwrap();
    let second = MediaRange::new(cut, duration).unwrap();
    assert!(!first.overlaps(second).unwrap(), "the cut is half-open");
    assert!(!first.contains(cut).unwrap());
    assert!(second.contains(cut).unwrap());
    assert!(first.contains(stamps[3]).unwrap());
    assert!(second.contains(stamps[4]).unwrap());
    assert!(cut
        .same_instant(Timestamp::new(1, Timebase::new(1, 5)))
        .unwrap());

    // Reject precision-lossy JSON numbers even when they fit the i64 range.
    let unsafe_pts = serde_json::json!({
        "pts": 9_007_199_254_740_992_u64,
        "timebase": {"num": 1, "den": 90_000}
    });
    assert!(serde_json::from_value::<Timestamp>(unsafe_pts).is_err());
}

#[test]
fn youtube_v1_and_speech_preserve_source_receipts_and_cut_semantics() {
    let fixture = fixture();
    let source = &fixture["syntheticSource"];
    let batch: MediaEvidenceBatchV1 = serde_json::from_value(fixture["batch"].clone()).unwrap();
    let media = MediaIdentity {
        asset_id: AssetId::new(source["assetId"].as_str().unwrap()).unwrap(),
        content_hash: ContentHash::from_bytes(
            source["contentBytesUtf8"].as_str().unwrap().as_bytes(),
        ),
        duration_seconds: Some(timestamp(&source["duration"]).seconds()),
    };
    assert_eq!(batch.video.id, source["videoId"].as_str().unwrap());
    assert_eq!(
        media.asset_id.as_str(),
        format!("youtube-corpus:video:{}", batch.video.id)
    );

    let frames = fixture["frameTimestamps"].as_array().unwrap();
    let cut = timestamp(&fixture["cut"]);
    for (index, scene) in batch.scenes.iter().enumerate() {
        let start = if index == 0 {
            timestamp(&frames[0]["timestamp"])
        } else {
            cut
        };
        let end = if index == 0 {
            cut
        } else {
            timestamp(&source["duration"])
        };
        MediaRange::new(start, end).unwrap();
        assert_eq!(scene.start_seconds, start.seconds());
        assert_eq!(scene.end_seconds, end.seconds());
        assert_eq!(scene.start_frame, if index == 0 { 0 } else { 4 });
        assert_eq!(scene.end_frame, if index == 0 { 4 } else { 7 });
    }
    for observation in &batch.ocr_observations {
        let index = observation.frame_index.unwrap() as usize;
        let exact = timestamp(&frames[index]["timestamp"]);
        assert_eq!(observation.timestamp_seconds, Some(exact.seconds()));
        assert_eq!(
            observation.scene_index,
            Some(if exact.chronological_cmp(cut).unwrap() == Ordering::Less {
                0
            } else {
                1
            })
        );
    }

    let speech: SpeechRun = serde_json::from_value(fixture["speechRun"].clone()).unwrap();
    for (segment, anchors) in speech
        .segments
        .iter()
        .zip(fixture["speechAnchors"].as_array().unwrap())
    {
        let start = timestamp(&anchors["start"]);
        let end = timestamp(&anchors["end"]);
        assert_eq!(segment.index, anchors["index"].as_u64().unwrap());
        assert_eq!(segment.start_seconds, start.seconds());
        assert_eq!(segment.end_seconds, end.seconds());
        MediaRange::new(start, end).unwrap();
    }

    let mut input = composition_input_from_batch(media.clone(), &batch).unwrap();
    input.speech = ModalityInput::Completed { runs: vec![speech] };
    let analysis = compose(&input).unwrap();
    assert_eq!(analysis.media, media);
    assert_eq!(
        serde_json::from_slice::<Value>(&serialized(&analysis)).unwrap()["schemaVersion"],
        1
    );
    assert_eq!(serialized(&analysis), serialized(&compose(&input).unwrap()));
    for modality in [Modality::Scene, Modality::Speech, Modality::VisibleText] {
        assert!(matches!(
            coverage(&analysis, modality),
            CoverageStatus::CompletedWithFindings { .. }
        ));
    }
    assert_eq!(analysis.evidence.len(), 6);
    for evidence in &analysis.evidence {
        assert_eq!(evidence.asset_id, media.asset_id);
        assert_eq!(evidence.content_hash, media.content_hash);
        assert_eq!(
            evidence.receipt.input_hash,
            "sha256:aa3adcebd323985cf4b77704b150c70188714fd5476d9e86c1a4c9fd84b1a63a"
        );
    }
    assert_eq!(
        item(&analysis, "obs:vfr-ocr-cut").start_seconds,
        cut.seconds()
    );
    assert_eq!(item(&analysis, "scene:vfr-sc-0").end_seconds, cut.seconds());
    assert_eq!(
        item(&analysis, "scene:vfr-sc-1").start_seconds,
        cut.seconds()
    );

    let relations = labelled_relations(&analysis);
    assert_eq!(
        relations,
        BTreeSet::from([
            (
                RelationKind::SceneSpeech,
                "scene:vfr-sc-0".into(),
                "speech:0".into()
            ),
            (
                RelationKind::SceneSpeech,
                "scene:vfr-sc-1".into(),
                "speech:1".into()
            ),
            (
                RelationKind::SceneVisibleText,
                "scene:vfr-sc-0".into(),
                "obs:vfr-ocr-before".into()
            ),
            (
                RelationKind::SceneVisibleText,
                "scene:vfr-sc-1".into(),
                "obs:vfr-ocr-cut".into()
            ),
            (
                RelationKind::SpeechVisibleTextCooccurrence,
                "speech:0".into(),
                "obs:vfr-ocr-before".into()
            ),
            (
                RelationKind::SpeechVisibleTextCooccurrence,
                "speech:1".into(),
                "obs:vfr-ocr-cut".into()
            ),
        ])
    );
}

fn exact_fixture_input() -> (
    media_intelligence_core::CompositionInput,
    MediaEvidenceBatchV1,
    ExactTimingSidecarV1,
) {
    let fixture = fixture();
    let source = &fixture["syntheticSource"];
    let media = MediaIdentity {
        asset_id: AssetId::new(source["assetId"].as_str().unwrap()).unwrap(),
        content_hash: ContentHash::from_bytes(
            source["contentBytesUtf8"].as_str().unwrap().as_bytes(),
        ),
        duration_seconds: Some(timestamp(&source["duration"]).seconds()),
    };
    let batch: MediaEvidenceBatchV1 = serde_json::from_value(fixture["batch"].clone()).unwrap();
    let speech: SpeechRun = serde_json::from_value(fixture["speechRun"].clone()).unwrap();
    let mut input = composition_input_from_batch(media.clone(), &batch).unwrap();
    input.speech = ModalityInput::Completed { runs: vec![speech] };

    let mut anchors = Vec::new();
    if let ModalityInput::Completed { runs } = &input.scenes {
        for run in runs {
            for scene in &run.scenes {
                let (start, end) = match scene.index {
                    0 => (
                        timestamp(&fixture["frameTimestamps"][0]["timestamp"]),
                        timestamp(&fixture["cut"]),
                    ),
                    1 => (timestamp(&fixture["cut"]), timestamp(&source["duration"])),
                    _ => unreachable!("fixture has two scenes"),
                };
                anchors.push(ExactAnchorV1 {
                    receipt: run.receipt.clone(),
                    record_id: scene.id.clone(),
                    timing: ExactAnchorTimingV1::Range { start, end },
                    frame_index: None,
                    start_frame: scene.start_frame,
                    end_frame: scene.end_frame,
                });
            }
        }
    }
    if let ModalityInput::Completed { runs } = &input.visible_text {
        for run in runs {
            for observation in &run.observations {
                let frame = observation.frame_index.unwrap() as usize;
                anchors.push(ExactAnchorV1 {
                    receipt: run.receipt.clone(),
                    record_id: observation.id.clone(),
                    timing: ExactAnchorTimingV1::Instant {
                        timestamp: timestamp(&fixture["frameTimestamps"][frame]["timestamp"]),
                    },
                    frame_index: observation.frame_index,
                    start_frame: None,
                    end_frame: None,
                });
            }
        }
    }
    if let ModalityInput::Completed { runs } = &input.speech {
        for run in runs {
            for segment in &run.segments {
                let speech = &fixture["speechAnchors"][segment.index as usize];
                anchors.push(ExactAnchorV1 {
                    receipt: run.receipt.clone(),
                    record_id: segment.index.to_string(),
                    timing: ExactAnchorTimingV1::Range {
                        start: timestamp(&speech["start"]),
                        end: timestamp(&speech["end"]),
                    },
                    frame_index: None,
                    start_frame: None,
                    end_frame: None,
                });
            }
        }
    }
    let sidecar = ExactTimingSidecarV1::new(
        media,
        Some(batch.revision.clone()),
        vec![Modality::Scene, Modality::VisibleText, Modality::Speech],
        anchors,
    )
    .unwrap();
    (input, batch, sidecar)
}

fn batch_with_original_frame_pts() -> (MediaIdentity, MediaEvidenceBatchV1) {
    let (input, mut batch, _) = exact_fixture_input();
    let fixture = fixture();
    for scene in &mut batch.scenes {
        let (start, end) = match scene.scene_index {
            0 => (
                timestamp(&fixture["frameTimestamps"][0]["timestamp"]),
                timestamp(&fixture["cut"]),
            ),
            1 => (
                timestamp(&fixture["cut"]),
                timestamp(&fixture["syntheticSource"]["duration"]),
            ),
            _ => unreachable!(),
        };
        scene.metadata["exactTiming"] = serde_json::json!({
            "schemaVersion": 1,
            "clock": "media_relative",
            "source": "ffprobe:frame-pts",
            "start": start,
            "end": end,
        });
        scene.provenance.processing_config["exactTiming"] =
            serde_json::json!("ffprobe:frame-pts");
    }
    for observation in &mut batch.ocr_observations {
        let frame_index = observation.frame_index.unwrap() as usize;
        observation.attributes["exactTiming"] = serde_json::json!({
            "schemaVersion": 1,
            "clock": "media_relative",
            "source": "ffprobe:frame-pts",
            "timestamp": timestamp(&fixture["frameTimestamps"][frame_index]["timestamp"]),
        });
        observation.provenance.processing_config["exactTiming"] =
            serde_json::json!("ffprobe:frame-pts");
    }
    (input.media, batch)
}

#[test]
fn corpus_v1_metadata_can_export_lossless_vfr_sidecar_without_schema_migration() {
    let (media, batch) = batch_with_original_frame_pts();
    let original = serde_json::to_vec(&batch).unwrap();
    let batch: MediaEvidenceBatchV1 = serde_json::from_slice(&original).unwrap();
    let sidecar = exact_timing_from_batch(media.clone(), &batch)
        .unwrap()
        .expect("original integer PTS recorded in v1 extension fields");
    assert_eq!(
        sidecar.base_revision.as_deref(),
        Some(batch.revision.as_str())
    );
    assert_eq!(
        sidecar.exact_modalities,
        vec![Modality::VisibleText, Modality::Scene]
    );
    assert_eq!(sidecar.anchors.len(), 4);
    let input = composition_input_from_batch(media, &batch).unwrap();
    let exact = compose_with_exact_timing(&input, sidecar).unwrap();
    let json = serde_json::to_vec(&exact).unwrap();
    let parsed: ExactEvidenceAnalysisV1 = serde_json::from_slice(&json).unwrap();
    assert_eq!(serde_json::to_vec(&parsed).unwrap(), json);
    assert_eq!(parsed.legacy, compose(&input).unwrap());

    let scene = parsed
        .exact_timing
        .anchors
        .iter()
        .find(|anchor| anchor.record_id == "vfr-sc-1")
        .unwrap();
    assert!(matches!(
        scene.timing,
        ExactAnchorTimingV1::Range { start, .. }
            if start.pts == 18_000 && start.timebase.den == 90_000
    ));
}

#[test]
fn corpus_exact_adapter_rejects_mixed_forged_and_misprojected_pts() {
    let (input, legacy, _) = exact_fixture_input();
    let media = input.media;
    assert!(exact_timing_from_batch(media.clone(), &legacy).unwrap().is_none());

    let (media, batch) = batch_with_original_frame_pts();
    let mut mixed = batch.clone();
    mixed.scenes[1]
        .metadata
        .as_object_mut()
        .unwrap()
        .remove("exactTiming");
    assert!(exact_timing_from_batch(media.clone(), &mixed)
        .unwrap_err()
        .to_string()
        .contains("mix"));

    let mut unproven = batch.clone();
    unproven.scenes[0]
        .provenance
        .processing_config
        .as_object_mut()
        .unwrap()
        .remove("exactTiming");
    assert!(exact_timing_from_batch(media.clone(), &unproven).is_err());

    let mut rounded = batch.clone();
    rounded.scenes[1].metadata["exactTiming"]["start"]["pts"] = serde_json::json!("18001");
    assert!(exact_timing_from_batch(media.clone(), &rounded).is_err());

    let mut unsafe_number = batch;
    unsafe_number.scenes[0].metadata["exactTiming"]["start"]["pts"] =
        serde_json::json!(9_007_199_254_740_992_u64);
    assert!(exact_timing_from_batch(media, &unsafe_number).is_err());
}

#[test]
fn exact_sidecar_preserves_producer_pts_across_v1_composition() {
    let (input, batch, sidecar) = exact_fixture_input();
    sidecar.validate_batch_revision(&batch.revision).unwrap();
    assert_eq!(sidecar.anchors.len(), 6);
    let exact = compose_with_exact_timing(&input, sidecar).unwrap();
    assert_eq!(
        serialized(&exact.legacy),
        serialized(&compose(&input).unwrap())
    );

    // Foundation's canonical decimal PTS and the *original* 90 kHz timebase
    // survive the independent, versioned sidecar wire unchanged.
    let json = serde_json::to_vec(&exact).unwrap();
    let roundtrip: ExactEvidenceAnalysisV1 = serde_json::from_slice(&json).unwrap();
    assert_eq!(serde_json::to_vec(&roundtrip).unwrap(), json);
    assert_eq!(roundtrip, exact);
    let scene = roundtrip
        .exact_timing
        .anchors
        .iter()
        .find(|anchor| anchor.record_id == "vfr-sc-1")
        .unwrap();
    let ExactAnchorTimingV1::Range { start, end } = scene.timing else {
        panic!("scene must have a range")
    };
    assert_eq!(
        (start.pts, start.timebase.num, start.timebase.den),
        (18_000, 1, 90_000)
    );
    assert_eq!(
        (end.pts, end.timebase.num, end.timebase.den),
        (36_000, 1, 90_000)
    );
    assert_eq!((scene.start_frame, scene.end_frame), (Some(4), Some(7)));
    assert_eq!(serde_json::to_value(start).unwrap()["pts"], "18000");
}

#[test]
fn exact_video_pts_can_coexist_with_honest_seconds_only_speech() {
    let (input, _, mut video_only) = exact_fixture_input();
    video_only
        .exact_modalities
        .retain(|modality| *modality != Modality::Speech);
    video_only
        .anchors
        .retain(|anchor| anchor.receipt.modality != Modality::Speech);
    let composed = compose_with_exact_timing(&input, video_only.clone()).unwrap();
    assert_eq!(composed.exact_timing.anchors.len(), 4);
    assert_eq!(
        composed.exact_timing.exact_modalities,
        vec![Modality::VisibleText, Modality::Scene]
    );
    assert_eq!(
        serialized(&composed.legacy),
        serialized(&compose(&input).unwrap())
    );

    // No producer can silently claim speech is exact if it has only seconds.
    video_only.exact_modalities.insert(0, Modality::Speech);
    assert!(compose_with_exact_timing(&input, video_only).is_err());
}

#[test]
fn exact_sidecar_rejects_missing_extra_or_misprojected_source_pts() {
    let (input, batch, sidecar) = exact_fixture_input();
    assert!(sidecar.validate_batch_revision("another-revision").is_err());
    assert!(sidecar.validate_batch_revision(&batch.revision).is_ok());

    let mut missing = sidecar.clone();
    missing.anchors.pop();
    assert!(compose_with_exact_timing(&input, missing).is_err());

    let mut duplicated = sidecar.clone();
    let duplicate = duplicated.anchors[0].clone();
    duplicated.anchors.push(duplicate);
    duplicated.anchors = ExactTimingSidecarV1::new(
        duplicated.media.clone(),
        duplicated.base_revision.clone(),
        duplicated.exact_modalities.clone(),
        duplicated.anchors,
    )
    .unwrap()
    .anchors;
    assert!(compose_with_exact_timing(&input, duplicated).is_err());

    let mut wrong_frame = sidecar.clone();
    let ocr = wrong_frame
        .anchors
        .iter_mut()
        .find(|anchor| anchor.record_id == "vfr-ocr-cut")
        .unwrap();
    ocr.frame_index = Some(3);
    assert!(compose_with_exact_timing(&input, wrong_frame).is_err());

    let mut invented = sidecar.clone();
    let scene = invented
        .anchors
        .iter_mut()
        .find(|anchor| anchor.record_id == "vfr-sc-1")
        .unwrap();
    if let ExactAnchorTimingV1::Range { start, .. } = &mut scene.timing {
        start.pts += 1;
    }
    assert!(compose_with_exact_timing(&input, invented).is_err());

    let mut invalid_clock = sidecar.clone();
    invalid_clock.clock = "unmapped_stream_pts".into();
    assert!(compose_with_exact_timing(&input, invalid_clock).is_err());

    let mut wrong_receipt = sidecar.clone();
    wrong_receipt.anchors[0].receipt.input_hash = "other-source".into();
    assert!(compose_with_exact_timing(&input, wrong_receipt).is_err());

    let mut wrong_media = sidecar;
    wrong_media.media.content_hash = ContentHash::from_bytes(b"another video");
    assert!(compose_with_exact_timing(&input, wrong_media).is_err());
}
