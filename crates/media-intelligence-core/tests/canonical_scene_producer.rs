//! Composes a real scenedetect-rs run (synthetic ffmpeg clip, real producer execution).
//! See tests/fixtures/README.md for the recorded commands, versions and hashes.

mod common;

use std::collections::BTreeSet;

use common::{coverage, fixture_text, labelled_relations};
use media_intelligence_core::{
    compose, AssetId, CompositionInput, ContentHash, CoverageStatus, EvidenceContent,
    MediaIdentity, Modality, ModalityInput, ProducerReceipt, RelationKind, SceneInterval, SceneRun,
    SpeechRun, TimedTextContract,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

const OUTPUT: &str = "scenedetect_rs_three_scenes.scenes.json";

fn producer_output() -> Value {
    serde_json::from_str(&fixture_text(OUTPUT)).unwrap()
}

fn provenance() -> Value {
    serde_json::from_str(&fixture_text("scenedetect_rs_three_scenes.provenance.json")).unwrap()
}

fn receipt() -> ProducerReceipt {
    serde_json::from_value(provenance()["receipt"].clone()).unwrap()
}

/// Test-only mapping of the `list-scenes --format json` document into a `SceneRun`.
/// Frame numbers are carried verbatim (the producer's 1-based inclusive numbering).
fn scene_run() -> SceneRun {
    let output = producer_output();
    let scenes = output["scenes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|scene| {
            let number = scene["scene_number"].as_u64().unwrap();
            SceneInterval {
                id: format!("scene-{number}"),
                index: number - 1,
                start_seconds: scene["start_seconds"].as_f64().unwrap(),
                end_seconds: scene["end_seconds"].as_f64().unwrap(),
                start_frame: scene["start_frame"].as_u64(),
                end_frame: scene["end_frame"].as_u64(),
            }
        })
        .collect();
    SceneRun {
        receipt: receipt(),
        scenes,
    }
}

fn media() -> MediaIdentity {
    let media = &provenance()["media"];
    MediaIdentity {
        asset_id: AssetId::new("acceptance:scenedetect-rs-three-scenes").unwrap(),
        content_hash: ContentHash::parse(media["sha256"].as_str().unwrap()).unwrap(),
        duration_seconds: media["durationSeconds"].as_f64(),
    }
}

fn input() -> CompositionInput {
    CompositionInput {
        media: media(),
        speech: ModalityInput::NotRequested,
        visible_text: ModalityInput::NotRequested,
        scenes: ModalityInput::Completed {
            runs: vec![scene_run()],
        },
    }
}

#[test]
fn committed_output_is_the_recorded_producer_bytes() {
    let digest = Sha256::digest(fixture_text(OUTPUT).as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, provenance()["producerOutputSha256"].as_str().unwrap());
    assert_eq!(receipt().modality, Modality::Scene);
}

#[test]
fn producer_scenes_compose_exactly() {
    let output = producer_output();
    let analysis = compose(&input()).unwrap();

    assert_eq!(output["scene_count"].as_u64(), Some(3));
    assert_eq!(analysis.evidence.len(), 3);
    for (evidence, scene) in analysis
        .evidence
        .iter()
        .zip(output["scenes"].as_array().unwrap())
    {
        assert_eq!(evidence.modality, Modality::Scene);
        assert_eq!(
            evidence.start_seconds,
            scene["start_seconds"].as_f64().unwrap()
        );
        assert_eq!(evidence.end_seconds, scene["end_seconds"].as_f64().unwrap());
        assert_eq!(evidence.receipt, receipt());
        assert_eq!(evidence.content_hash, media().content_hash);
        let EvidenceContent::Scene(content) = &evidence.content else {
            panic!("scene")
        };
        assert_eq!(
            content.scene_index + 1,
            scene["scene_number"].as_u64().unwrap()
        );
        assert_eq!(content.start_frame, scene["start_frame"].as_u64());
        assert_eq!(content.end_frame, scene["end_frame"].as_u64());
    }
    let bounds: Vec<_> = analysis
        .evidence
        .iter()
        .map(|e| (e.start_seconds, e.end_seconds))
        .collect();
    assert_eq!(bounds, [(0.0, 5.0), (5.0, 12.0), (12.0, 16.0)]);

    assert_eq!(
        coverage(&analysis, Modality::Scene),
        &CoverageStatus::CompletedWithFindings {
            receipts: vec![receipt()],
            evidence_count: 3,
        }
    );
    assert_eq!(
        coverage(&analysis, Modality::Speech),
        &CoverageStatus::NotRequested
    );
}

#[test]
fn producer_scenes_and_timed_text_share_one_timeline() {
    let transcript: TimedTextContract =
        serde_json::from_str(&fixture_text("timed_text_transcript.json")).unwrap();
    let speech_receipt: ProducerReceipt = serde_json::from_value(serde_json::json!({
        "modality": "speech",
        "producer": "audio-analysis",
        "producerVersion": "cc8075f633a4bda5d1b9902b3f4c4d07eca017d9",
        "model": "whisper-base.en",
        "modelVersion": "ggml-base.en",
        "configHash": "sha256:5f1e0c9a",
        "inputHash": "sha256:03e3ba48bef6b67535d18c24a1c8e7dbfaf0f9fad9b17dcd1918add385ba7279"
    }))
    .unwrap();
    let mut input = input();
    input.speech = ModalityInput::Completed {
        runs: vec![SpeechRun::from_timed_text(speech_receipt, &transcript).unwrap()],
    };
    let analysis = compose(&input).unwrap();

    for pair in analysis.evidence.windows(2) {
        assert!(pair[0].start_seconds <= pair[1].start_seconds);
    }
    let scene_speech: BTreeSet<_> = labelled_relations(&analysis)
        .into_iter()
        .filter(|(kind, _, _)| *kind == RelationKind::SceneSpeech)
        .map(|(_, scene, speech)| (scene, speech))
        .collect();
    let expected: BTreeSet<_> = [
        ("scene:scene-1", "speech:0"), // [0,5) ∩ [0,4.2)
        ("scene:scene-1", "speech:1"), // [0,5) ∩ [4.2,9.8)
        ("scene:scene-2", "speech:1"), // [5,12) ∩ [4.2,9.8)
        ("scene:scene-3", "speech:2"), // [12,16) ∩ [12.5,16)
    ]
    .into_iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect();
    assert_eq!(scene_speech, expected);
}
