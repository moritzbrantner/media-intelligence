//! Portable synthetic VFR fixture: Foundation exact PTS, YouTube Corpus v1,
//! scene/OCR adapters, and Media Intelligence v1 composition.
//!
//! No decoder is exercised here. This is the representation and integration
//! boundary test; a real-media replay is a separate acceptance requirement.

mod common;

use std::cmp::Ordering;
use std::collections::BTreeSet;

use common::{coverage, fixture_text, item, labelled_relations, serialized};
use media_intelligence_core::compat::youtube_corpus::{
    composition_input_from_batch, MediaEvidenceBatchV1,
};
use media_intelligence_core::{
    compose, AssetId, ContentHash, CoverageStatus, MediaIdentity, Modality, ModalityInput,
    RelationKind, SpeechRun,
};
use moenarch_media_core::{MediaRange, Timestamp};
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
    assert!(cut.same_instant(moenarch_media_core::Timestamp::new(
        1, moenarch_media_core::Timebase::new(1, 5)
    )).unwrap());

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
    assert_eq!(serde_json::from_slice::<Value>(&serialized(&analysis)).unwrap()["schemaVersion"], 1);
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
    assert_eq!(item(&analysis, "obs:vfr-ocr-cut").start_seconds, cut.seconds());
    assert_eq!(item(&analysis, "scene:vfr-sc-0").end_seconds, cut.seconds());
    assert_eq!(item(&analysis, "scene:vfr-sc-1").start_seconds, cut.seconds());

    let relations = labelled_relations(&analysis);
    assert_eq!(
        relations,
        BTreeSet::from([
            (RelationKind::SceneSpeech, "scene:vfr-sc-0".into(), "speech:0".into()),
            (RelationKind::SceneSpeech, "scene:vfr-sc-1".into(), "speech:1".into()),
            (RelationKind::SceneVisibleText, "scene:vfr-sc-0".into(), "obs:vfr-ocr-before".into()),
            (RelationKind::SceneVisibleText, "scene:vfr-sc-1".into(), "obs:vfr-ocr-cut".into()),
            (RelationKind::SpeechVisibleTextCooccurrence, "speech:0".into(), "obs:vfr-ocr-before".into()),
            (RelationKind::SpeechVisibleTextCooccurrence, "speech:1".into(), "obs:vfr-ocr-cut".into()),
        ])
    );
}
