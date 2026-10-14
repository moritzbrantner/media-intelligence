//! YouTube Corpus `MediaEvidenceBatchV1` and Foundation timed-text inputs compose without
//! losing producer ids or provenance.

mod common;

use std::collections::BTreeSet;

use common::{coverage, fixture_text, item, labelled_relations};
use media_intelligence_core::compat::youtube_corpus::{
    composition_input_from_batch, MediaEvidenceBatchV1, ProcessingEvidenceV1,
};
use media_intelligence_core::{
    compose, AssetId, ContentHash, CoverageStatus, EvidenceContent, MediaIdentity, Modality,
    ModalityInput, ProducerReceipt, RelationKind, SpeechRun, TimedTextContract,
};

fn batch() -> MediaEvidenceBatchV1 {
    serde_json::from_str(&fixture_text("youtube_corpus_media_evidence_batch_v1.json"))
        .expect("fixture has the MediaEvidenceBatchV1 shape")
}

fn media() -> MediaIdentity {
    MediaIdentity {
        asset_id: AssetId::new("youtube-corpus:video:7d0b6c2e-4f1a-4c3e-9a51-2b8f6d0e7c14")
            .unwrap(),
        content_hash: ContentHash::from_bytes(b"Xq3pL9vT2aE retained media bytes"),
        duration_seconds: Some(24.0),
    }
}

fn assert_receipt_matches(
    receipt: &ProducerReceipt,
    modality: Modality,
    provenance: &ProcessingEvidenceV1,
) {
    assert_eq!(receipt.modality, modality);
    assert_eq!(receipt.producer, provenance.processor);
    assert_eq!(receipt.producer_version, provenance.processor_version);
    assert_eq!(receipt.model, provenance.model);
    assert_eq!(receipt.model_version, provenance.model_version);
    assert_eq!(receipt.config_hash, provenance.config_hash);
    assert_eq!(receipt.input_hash, provenance.input_hash);
    assert_eq!(receipt.run_id.as_deref(), Some(provenance.run_id.as_str()));
    assert_eq!(receipt.config.as_ref(), Some(&provenance.processing_config));
}

#[test]
fn batch_fixture_round_trips_through_the_mirrored_shape() {
    let text = fixture_text("youtube_corpus_media_evidence_batch_v1.json");
    let original: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(serde_json::to_value(batch()).unwrap(), original);
}

#[test]
fn batch_converts_into_runs_with_full_provenance() {
    let batch = batch();
    let input = composition_input_from_batch(media(), &batch).unwrap();

    assert_eq!(input.media, media());
    assert_eq!(
        input.speech,
        ModalityInput::Unprocessed,
        "the batch cannot express speech"
    );

    let ModalityInput::Completed { runs: scene_runs } = &input.scenes else {
        panic!("scenes")
    };
    assert_eq!(scene_runs.len(), 1);
    assert_receipt_matches(
        &scene_runs[0].receipt,
        Modality::Scene,
        &batch.scenes[0].provenance,
    );
    let scene_ids: BTreeSet<_> = scene_runs[0].scenes.iter().map(|s| s.id.clone()).collect();
    let batch_scene_ids: BTreeSet<_> = batch.scenes.iter().map(|s| s.id.clone()).collect();
    assert_eq!(scene_ids, batch_scene_ids);

    let ModalityInput::Completed { runs: ocr_runs } = &input.visible_text else {
        panic!("ocr")
    };
    assert_eq!(ocr_runs.len(), 1);
    assert_receipt_matches(
        &ocr_runs[0].receipt,
        Modality::VisibleText,
        &batch.ocr_observations[0].provenance,
    );
    let observation_ids: BTreeSet<_> = ocr_runs[0]
        .observations
        .iter()
        .map(|o| o.id.clone())
        .collect();
    let batch_observation_ids: BTreeSet<_> = batch
        .ocr_observations
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(observation_ids, batch_observation_ids);
    assert_eq!(ocr_runs[0].tracks.len(), 1);
    assert_eq!(ocr_runs[0].tracks[0].id, batch.ocr_tracks[0].id);
    assert_eq!(
        ocr_runs[0].tracks[0].observation_ids,
        batch.ocr_tracks[0].observation_ids
    );
    assert_eq!(
        ocr_runs[0].tracks[0].role.as_deref(),
        Some("presentation_slide")
    );
}

#[test]
fn batch_composes_without_losing_ids_or_provenance() {
    let batch = batch();
    let analysis = compose(&composition_input_from_batch(media(), &batch).unwrap()).unwrap();

    assert_eq!(
        coverage(&analysis, Modality::Speech),
        &CoverageStatus::Unprocessed
    );
    for scene in &batch.scenes {
        let evidence = item(&analysis, &format!("scene:{}", scene.id));
        assert_eq!(
            (evidence.start_seconds, evidence.end_seconds),
            (scene.start_seconds, scene.end_seconds)
        );
        assert_receipt_matches(&evidence.receipt, Modality::Scene, &scene.provenance);
        let EvidenceContent::Scene(content) = &evidence.content else {
            panic!()
        };
        assert_eq!(content.scene_index, scene.scene_index);
        assert_eq!(
            (content.start_frame, content.end_frame),
            (Some(scene.start_frame), Some(scene.end_frame))
        );
    }

    let track = &batch.ocr_tracks[0];
    let evidence = item(&analysis, &format!("track:{}", track.id));
    assert_receipt_matches(&evidence.receipt, Modality::VisibleText, &track.provenance);
    assert_eq!((evidence.start_seconds, evidence.end_seconds), (1.0, 3.0));
    let EvidenceContent::VisibleText(content) = &evidence.content else {
        panic!()
    };
    let ids: Vec<_> = content
        .observations
        .iter()
        .map(|o| o.observation_id.clone())
        .collect();
    assert_eq!(ids, track.observation_ids);

    let untracked = &batch.ocr_observations[3];
    let evidence = item(&analysis, &format!("obs:{}", untracked.id));
    assert_eq!((evidence.start_seconds, evidence.end_seconds), (14.0, 14.0));

    // 2 scenes + 1 track + 1 untracked observation; no OCR frame inflation.
    assert_eq!(analysis.evidence.len(), 4);
}

#[test]
fn empty_batch_lists_are_unprocessed_not_completed_empty() {
    let mut batch = batch();
    batch.scenes.clear();
    batch.ocr_observations.clear();
    batch.ocr_tracks.clear();
    let input = composition_input_from_batch(media(), &batch).unwrap();
    assert_eq!(input.scenes, ModalityInput::Unprocessed);
    assert_eq!(input.visible_text, ModalityInput::Unprocessed);
}

#[test]
fn foundation_timed_text_and_batch_compose_into_one_timeline() {
    let transcript: TimedTextContract =
        serde_json::from_str(&fixture_text("timed_text_transcript.json")).unwrap();
    let receipt: ProducerReceipt = serde_json::from_value(serde_json::json!({
        "modality": "speech",
        "producer": "audio-analysis",
        "producerVersion": "cc8075f633a4bda5d1b9902b3f4c4d07eca017d9",
        "model": "whisper-base.en",
        "modelVersion": "ggml-base.en",
        "configHash": "sha256:5f1e0c9a",
        "inputHash": "fnv1a64:c4e1b7a09d2f3e58",
        "runId": "run-speech-yt-0001"
    }))
    .unwrap();
    let run = SpeechRun::from_timed_text(receipt.clone(), &transcript).unwrap();
    assert_eq!(run.receipt, receipt);
    let segments: Vec<_> = run
        .segments
        .iter()
        .map(|s| (s.index, s.start_seconds, s.end_seconds, s.text.as_str()))
        .collect();
    assert_eq!(
        segments,
        [
            (0, 0.0, 4.2, "Today we look inside SQLite."),
            (1, 4.2, 9.8, "Every database is a single file."),
            (2, 12.5, 16.0, "The source is on GitHub."),
        ]
    );
    // Foundation stores segment confidence as f32; the value is widened, not rescaled.
    assert_eq!(run.segments[0].confidence, Some(f64::from(0.93_f32)));
    assert_eq!(run.segments[0].language.as_deref(), Some("en"));

    let batch = batch();
    let mut input = composition_input_from_batch(media(), &batch).unwrap();
    input.speech = ModalityInput::Completed { runs: vec![run] };
    let analysis = compose(&input).unwrap();

    assert!(matches!(
        coverage(&analysis, Modality::Speech),
        CoverageStatus::CompletedWithFindings {
            evidence_count: 3,
            ..
        }
    ));
    let relations = labelled_relations(&analysis);
    let track = format!("track:{}", batch.ocr_tracks[0].id);
    let url = format!("obs:{}", batch.ocr_observations[3].id);
    let scene_1 = format!("scene:{}", batch.scenes[1].id);
    for expected in [
        (
            RelationKind::SpeechVisibleTextCooccurrence,
            "speech:0".to_string(),
            track,
        ),
        (
            RelationKind::SpeechVisibleTextCooccurrence,
            "speech:2".to_string(),
            url,
        ),
        (RelationKind::SceneSpeech, scene_1, "speech:2".to_string()),
    ] {
        assert!(relations.contains(&expected), "missing {expected:?}");
    }
    let co_occurrences = relations
        .iter()
        .filter(|r| r.0 == RelationKind::SpeechVisibleTextCooccurrence)
        .count();
    assert_eq!(co_occurrences, 2);
}
