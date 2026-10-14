//! #33 seam (default features): stages run speech -> scenes -> visible text, the
//! visible-text producer receives the scene run of the same analysis, and a producer that
//! requires scenes fails without one and is keyed under `scene_bound_identity`.
//!
//! Scenes are real (visual-analysis on a synthetic ffmpeg clip). The visible-text producers
//! here are acceptance-written fakes; no OCR runs.

mod common;

use std::cell::{Cell, RefCell};
use std::fs;

use common::*;
use media_intelligence_analysis::{
    analyze, input_hash, scene_bound_identity, AnalysisProfile, AnalysisRequest, AssetId,
    CancellationToken, ContentHash, InMemoryStageCache, MediaIdentity, Modality, ProducerError,
    ProducerIdentity, ProducerInput, ProducerReceipt, Producers, SceneConfig, SceneRun, StageKey,
    StageStatus, VisibleTextProducer, VisibleTextRun,
};
use media_intelligence_core::{CoverageStatus, OcrObservation, SceneInterval};

/// Visible-text fake that records the scene run it was handed.
struct RecordingOcr {
    identity: ProducerIdentity,
    requires_scenes: bool,
    /// Receipt identity: bound to the received scenes (the #33 rule) or the plain identity.
    bind_receipt: bool,
    calls: Cell<usize>,
    received: RefCell<Vec<Option<SceneRun>>>,
}

impl RecordingOcr {
    fn generic(config_hash: &str) -> Self {
        Self {
            identity: identity("recording-ocr", config_hash),
            requires_scenes: false,
            bind_receipt: false,
            calls: Cell::new(0),
            received: RefCell::new(Vec::new()),
        }
    }

    fn scene_dependent(config_hash: &str) -> Self {
        Self {
            identity: identity("scene-dependent-ocr", config_hash),
            requires_scenes: true,
            bind_receipt: true,
            ..Self::generic(config_hash)
        }
    }
}

impl VisibleTextProducer for RecordingOcr {
    fn identity(&self) -> ProducerIdentity {
        self.identity.clone()
    }

    fn requires_scenes(&self) -> bool {
        self.requires_scenes
    }

    fn run(&self, input: &ProducerInput<'_>) -> Result<VisibleTextRun, ProducerError> {
        self.calls.set(self.calls.get() + 1);
        self.received.borrow_mut().push(input.scenes.cloned());
        let receipt_identity = match (self.bind_receipt, input.scenes) {
            (true, Some(scenes)) => scene_bound_identity(&self.identity, scenes),
            _ => self.identity.clone(),
        };
        Ok(VisibleTextRun {
            receipt: ProducerReceipt {
                modality: Modality::VisibleText,
                producer: receipt_identity.producer,
                producer_version: receipt_identity.producer_version,
                model: receipt_identity.model,
                model_version: receipt_identity.model_version,
                config_hash: receipt_identity.config_hash,
                input_hash: input.input_hash.to_string(),
                run_id: None,
                config: None,
            },
            observations: vec![OcrObservation {
                id: "o1".to_string(),
                text: "GREEN SLIDE".to_string(),
                timestamp_seconds: 8.0,
                frame_index: Some(200),
                language: None,
                region: None,
                confidence: None,
            }],
            tracks: vec![],
        })
    }
}

fn speech_scenes_ocr() -> AnalysisProfile {
    everything()
}

fn ocr_only() -> AnalysisProfile {
    AnalysisProfile {
        speech: false,
        visible_text: true,
        scenes: None,
    }
}

fn ocr_and_scenes(config: SceneConfig) -> AnalysisProfile {
    AnalysisProfile {
        speech: false,
        visible_text: true,
        scenes: Some(config),
    }
}

fn scene_receipt(status: &StageStatus) -> ProducerReceipt {
    match status {
        StageStatus::CompletedWithFindings { receipt, .. }
        | StageStatus::Reused { receipt, .. } => receipt.clone(),
        other => panic!("scenes did not complete: {other:?}"),
    }
}

fn failed_reason(status: &StageStatus) -> String {
    match status {
        StageStatus::Failed { reason, .. } => reason.clone(),
        other => panic!("expected Failed, got {other:?}"),
    }
}

fn receipt_identity(receipt: &ProducerReceipt) -> ProducerIdentity {
    ProducerIdentity {
        producer: receipt.producer.clone(),
        producer_version: receipt.producer_version.clone(),
        model: receipt.model.clone(),
        model_version: receipt.model_version.clone(),
        config_hash: receipt.config_hash.clone(),
    }
}

fn scene_run(config_hash: &str) -> SceneRun {
    SceneRun {
        receipt: ProducerReceipt {
            modality: Modality::Scene,
            producer: "visual-analysis".to_string(),
            producer_version: "rev".to_string(),
            model: "scenedetect-core:content".to_string(),
            model_version: "0.1.0".to_string(),
            config_hash: config_hash.to_string(),
            input_hash: format!("sha256:{}", "a".repeat(64)),
            run_id: None,
            config: None,
        },
        scenes: vec![SceneInterval {
            id: "s0".to_string(),
            index: 0,
            start_seconds: 0.0,
            end_seconds: 1.0,
            start_frame: Some(0),
            end_frame: Some(25),
        }],
    }
}

// ---------------------------------------------------------------------------------------
// scene_bound_identity
// ---------------------------------------------------------------------------------------

#[test]
fn scene_bound_identity_replaces_only_the_config_hash_deterministically() {
    let own = identity("ocr", "sha256:own-a");
    let bound = scene_bound_identity(&own, &scene_run("sha256:scene-a"));
    assert_eq!(bound.producer, own.producer);
    assert_eq!(bound.producer_version, own.producer_version);
    assert_eq!(bound.model, own.model);
    assert_eq!(bound.model_version, own.model_version);
    let hex = bound
        .config_hash
        .strip_prefix("sha256:")
        .expect("sha256: prefix");
    assert_eq!(hex.len(), 64);
    assert!(hex
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert_ne!(bound.config_hash, own.config_hash);
    assert_eq!(
        bound,
        scene_bound_identity(&own, &scene_run("sha256:scene-a")),
        "deterministic"
    );
    // A different scene configuration or a different own configuration changes it.
    let other_scenes = scene_bound_identity(&own, &scene_run("sha256:scene-b"));
    let other_own = scene_bound_identity(
        &identity("ocr", "sha256:own-b"),
        &scene_run("sha256:scene-a"),
    );
    assert_ne!(other_scenes.config_hash, bound.config_hash);
    assert_ne!(other_own.config_hash, bound.config_hash);
    assert_ne!(other_scenes.config_hash, other_own.config_hash);
}

// ---------------------------------------------------------------------------------------
// Stage order and scene hand-off (any visible-text producer)
// ---------------------------------------------------------------------------------------

#[test]
fn visible_text_producer_receives_the_scene_run_completed_by_the_same_analysis() {
    let Some(clip) = three_scene_clip("order_scene_handoff") else {
        return;
    };
    let speech = FakeSpeech::new(Behaviour::Succeed);
    let ocr = RecordingOcr::generic("sha256:recording-a");
    let report = analyze(
        &request(&clip, speech_scenes_ocr()),
        &Producers {
            speech: Some(&speech),
            visible_text: Some(&ocr),
        },
        &mut InMemoryStageCache::new(),
        &CancellationToken::new(),
    )
    .unwrap();

    assert!(matches!(
        report.scenes,
        StageStatus::CompletedWithFindings {
            evidence_count: 3,
            ..
        }
    ));
    assert_eq!(ocr.calls.get(), 1);
    let received = ocr.received.borrow();
    let handed = received[0]
        .as_ref()
        .expect("scenes ran before visible text and were handed over");
    // Exactly the scene run this analysis composed.
    assert_eq!(handed, &scene_run_from_evidence(&report.analysis));
    assert_eq!(handed.receipt, scene_receipt(&report.scenes));
    assert_eq!(handed.scenes.len(), 3);
    // A producer that does not require scenes keeps its plain #27 identity.
    match &report.visible_text {
        StageStatus::CompletedWithFindings { receipt, .. } => {
            assert_eq!(receipt_identity(receipt), ocr.identity)
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn cancellation_by_the_visible_text_producer_keeps_the_scenes_that_already_ran() {
    // Old (#27 slice 1) order ran visible text before scenes, so scenes would have been
    // Cancelled here. With speech -> scenes -> visible text they have completed.
    let Some(clip) = three_scene_clip("order_ocr_cancels_last") else {
        return;
    };
    let speech = FakeSpeech::new(Behaviour::Succeed);
    let ocr = FakeOcr::new(Behaviour::CancelThenStop);
    let token = CancellationToken::new();
    let report = analyze(
        &request(&clip, speech_scenes_ocr()),
        &Producers {
            speech: Some(&speech),
            visible_text: Some(&ocr),
        },
        &mut InMemoryStageCache::new(),
        &token,
    )
    .unwrap();

    assert!(token.is_cancelled());
    assert_eq!(speech.calls.get(), 1);
    assert_eq!(ocr.calls.get(), 1);
    assert!(matches!(
        report.speech,
        StageStatus::CompletedWithFindings { .. }
    ));
    assert!(
        matches!(
            report.scenes,
            StageStatus::CompletedWithFindings {
                evidence_count: 3,
                ..
            }
        ),
        "scenes run before visible text: {:?}",
        report.scenes
    );
    assert_eq!(report.visible_text, StageStatus::Cancelled);
    assert_eq!(count(&report.analysis, Modality::Scene), 3);
    assert_eq!(count(&report.analysis, Modality::VisibleText), 0);
}

#[test]
fn visible_text_producer_receives_a_reused_scene_run() {
    let Some(clip) = three_scene_clip("order_reused_scenes") else {
        return;
    };
    let mut cache = InMemoryStageCache::new();
    let first = analyze(
        &request(&clip, scenes_only()),
        &Producers::default(),
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    let ocr = RecordingOcr::generic("sha256:recording-a");
    let second = analyze(
        &request(&clip, ocr_and_scenes(SceneConfig::default())),
        &Producers {
            speech: None,
            visible_text: Some(&ocr),
        },
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(second.scenes, StageStatus::Reused { .. }));
    let received = ocr.received.borrow();
    assert_eq!(
        received[0].as_ref(),
        Some(&scene_run_from_evidence(&first.analysis)),
        "the reused run is handed over"
    );
}

#[test]
fn without_requested_scenes_a_generic_producer_receives_none_and_still_runs() {
    let Some(clip) = three_scene_clip("order_no_scenes_generic") else {
        return;
    };
    let ocr = RecordingOcr::generic("sha256:recording-a");
    let report = analyze(
        &request(&clip, ocr_only()),
        &Producers {
            speech: None,
            visible_text: Some(&ocr),
        },
        &mut InMemoryStageCache::new(),
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(*ocr.received.borrow(), vec![None]);
    assert!(matches!(
        report.visible_text,
        StageStatus::CompletedWithFindings { .. }
    ));
}

// ---------------------------------------------------------------------------------------
// Scene-dependent producers (requires_scenes = true)
// ---------------------------------------------------------------------------------------

#[test]
fn scene_dependent_producer_fails_with_an_actionable_reason_when_scenes_are_not_requested() {
    let Some(clip) = three_scene_clip("order_dependent_not_requested") else {
        return;
    };
    let ocr = RecordingOcr::scene_dependent("sha256:dependent-a");
    let mut cache = InMemoryStageCache::new();
    let report = analyze(
        &request(&clip, ocr_only()),
        &Producers {
            speech: None,
            visible_text: Some(&ocr),
        },
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(ocr.calls.get(), 0, "no run without the scene run");
    let reason = failed_reason(&report.visible_text);
    assert!(
        reason.to_lowercase().contains("scene"),
        "the reason names the scene stage: {reason}"
    );
    assert_eq!(report.scenes, StageStatus::NotRequested);
    match coverage(&report.analysis, Modality::VisibleText) {
        CoverageStatus::Failed {
            reason: covered, ..
        } => assert_eq!(covered, &reason),
        other => panic!("{other:?}"),
    }
    assert!(
        cache.is_empty(),
        "no hidden scene detection, nothing cached"
    );
}

#[test]
fn scene_dependent_producer_fails_when_the_scene_stage_failed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("not-a-video.bin");
    fs::write(&path, b"plain bytes, not a video container").unwrap();
    let content_hash = ContentHash::from_bytes(fs::read(&path).unwrap());
    let request = AnalysisRequest {
        media: MediaIdentity {
            asset_id: AssetId::new("acceptance:issue-33-not-a-video").unwrap(),
            content_hash,
            duration_seconds: None,
        },
        media_path: path,
        profile: ocr_and_scenes(SceneConfig::default()),
    };
    let ocr = RecordingOcr::scene_dependent("sha256:dependent-a");
    let report = analyze(
        &request,
        &Producers {
            speech: None,
            visible_text: Some(&ocr),
        },
        &mut InMemoryStageCache::new(),
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(
        matches!(report.scenes, StageStatus::Failed { .. }),
        "{:?}",
        report.scenes
    );
    assert_eq!(ocr.calls.get(), 0);
    let reason = failed_reason(&report.visible_text);
    assert!(reason.to_lowercase().contains("scene"), "{reason}");
}

#[test]
fn scene_dependent_producer_is_keyed_and_receipted_under_the_scene_bound_identity() {
    let Some(clip) = three_scene_clip("order_dependent_bound") else {
        return;
    };
    let ocr = RecordingOcr::scene_dependent("sha256:dependent-a");
    let mut cache = InMemoryStageCache::new();
    let report = analyze(
        &request(&clip, ocr_and_scenes(SceneConfig::default())),
        &Producers {
            speech: None,
            visible_text: Some(&ocr),
        },
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    let scenes = scene_run_from_evidence(&report.analysis);
    let bound = scene_bound_identity(&ocr.identity, &scenes);
    match &report.visible_text {
        StageStatus::CompletedWithFindings {
            receipt,
            evidence_count: 1,
        } => {
            assert_eq!(receipt_identity(receipt), bound);
            assert_eq!(receipt.input_hash, input_hash(&clip.content_hash));
        }
        other => panic!("{other:?}"),
    }
    assert!(cache.keys().contains(&StageKey {
        modality: Modality::VisibleText,
        content_hash: clip.content_hash.clone(),
        producer: bound,
    }));

    // Unchanged rerun: both stages reused, the producer is not invoked again.
    let rerun = analyze(
        &request(&clip, ocr_and_scenes(SceneConfig::default())),
        &Producers {
            speech: None,
            visible_text: Some(&ocr),
        },
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(rerun.scenes, StageStatus::Reused { .. }));
    assert!(matches!(
        rerun.visible_text,
        StageStatus::Reused {
            evidence_count: 1,
            ..
        }
    ));
    assert_eq!(ocr.calls.get(), 1);
    assert_eq!(rerun.analysis, report.analysis);
}

#[test]
fn a_different_scene_configuration_re_executes_the_scene_dependent_producer() {
    let Some(clip) = three_scene_clip("order_dependent_scene_change") else {
        return;
    };
    let ocr = RecordingOcr::scene_dependent("sha256:dependent-a");
    let mut cache = InMemoryStageCache::new();
    let first = analyze(
        &request(&clip, ocr_and_scenes(SceneConfig::default())),
        &Producers {
            speech: None,
            visible_text: Some(&ocr),
        },
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    let changed = SceneConfig {
        min_scene_len: 200,
        ..SceneConfig::default()
    };
    let second = analyze(
        &request(&clip, ocr_and_scenes(changed)),
        &Producers {
            speech: None,
            visible_text: Some(&ocr),
        },
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        second.scenes,
        StageStatus::CompletedWithFindings {
            evidence_count: 1,
            ..
        }
    ));
    assert_eq!(
        ocr.calls.get(),
        2,
        "a cached run for other scenes is not reused"
    );
    let received = ocr.received.borrow();
    assert_eq!(received[1].as_ref().map(|run| run.scenes.len()), Some(1));
    let first_hash = match &first.visible_text {
        StageStatus::CompletedWithFindings { receipt, .. } => receipt.config_hash.clone(),
        other => panic!("{other:?}"),
    };
    match &second.visible_text {
        StageStatus::CompletedWithFindings { receipt, .. } => {
            assert_ne!(receipt.config_hash, first_hash)
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn scene_dependent_receipt_with_the_unbound_identity_is_rejected() {
    let Some(clip) = three_scene_clip("order_dependent_unbound_receipt") else {
        return;
    };
    let ocr = RecordingOcr {
        bind_receipt: false,
        ..RecordingOcr::scene_dependent("sha256:dependent-a")
    };
    let mut cache = InMemoryStageCache::new();
    let report = analyze(
        &request(&clip, ocr_and_scenes(SceneConfig::default())),
        &Producers {
            speech: None,
            visible_text: Some(&ocr),
        },
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(ocr.calls.get(), 1);
    assert!(
        matches!(report.visible_text, StageStatus::Failed { .. }),
        "{:?}",
        report.visible_text
    );
    assert_eq!(count(&report.analysis, Modality::VisibleText), 0);
    assert!(matches!(
        report.scenes,
        StageStatus::CompletedWithFindings { .. }
    ));
}

#[test]
fn cancelled_before_the_run_a_scene_dependent_producer_is_cancelled_not_failed() {
    let Some(clip) = three_scene_clip("order_dependent_cancelled") else {
        return;
    };
    let ocr = RecordingOcr::scene_dependent("sha256:dependent-a");
    let token = CancellationToken::new();
    token.cancel();
    for profile in [ocr_and_scenes(SceneConfig::default()), ocr_only()] {
        let report = analyze(
            &request(&clip, profile),
            &Producers {
                speech: None,
                visible_text: Some(&ocr),
            },
            &mut InMemoryStageCache::new(),
            &token,
        )
        .unwrap();
        assert_eq!(report.visible_text, StageStatus::Cancelled);
    }
    assert_eq!(ocr.calls.get(), 0);
}
