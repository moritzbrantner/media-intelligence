//! Idempotent stage reuse through the caller-owned cache.

mod common;

use common::*;
use media_intelligence_analysis::{
    analyze, CachedStage, CancellationToken, InMemoryStageCache, Modality, ProducerIdentity,
    Producers, SceneConfig, StageCache, StageKey, StageStatus, SCENE_MODEL, SCENE_MODEL_VERSION,
    SCENE_PRODUCER, VISUAL_ANALYSIS_REVISION,
};
use media_intelligence_core::{SceneInterval, SceneRun};

fn scene_key(
    content_hash: &media_intelligence_analysis::ContentHash,
    config: &SceneConfig,
) -> StageKey {
    StageKey {
        modality: Modality::Scene,
        content_hash: content_hash.clone(),
        producer: ProducerIdentity {
            producer: SCENE_PRODUCER.to_string(),
            producer_version: VISUAL_ANALYSIS_REVISION.to_string(),
            model: SCENE_MODEL.to_string(),
            model_version: SCENE_MODEL_VERSION.to_string(),
            config_hash: config.config_hash(),
        },
    }
}

#[test]
fn unchanged_rerun_reuses_every_stage_without_invoking_producers() {
    let Some(clip) = three_scene_clip("unchanged_rerun") else {
        return;
    };
    let speech = FakeSpeech::new(Behaviour::Succeed);
    let ocr = FakeOcr::new(Behaviour::Succeed);
    let producers = Producers {
        speech: Some(&speech),
        visible_text: Some(&ocr),
    };
    let mut cache = InMemoryStageCache::new();
    let first = analyze(
        &request(&clip, everything()),
        &producers,
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(cache.len(), 3, "one entry per completed stage");
    let keys = cache.keys();
    assert!(keys.contains(&scene_key(&clip.content_hash, &SceneConfig::default())));
    assert!(keys.contains(&StageKey {
        modality: Modality::Speech,
        content_hash: clip.content_hash.clone(),
        producer: speech.identity.clone(),
    }));
    assert!(keys.contains(&StageKey {
        modality: Modality::VisibleText,
        content_hash: clip.content_hash.clone(),
        producer: ocr.identity.clone(),
    }));

    let second = analyze(
        &request(&clip, everything()),
        &producers,
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(speech.calls.get(), 1, "speech producer not re-invoked");
    assert_eq!(ocr.calls.get(), 1, "OCR producer not re-invoked");
    for (status, count) in [
        (&second.speech, 4),
        (&second.visible_text, 3),
        (&second.scenes, 3),
    ] {
        match status {
            StageStatus::Reused { evidence_count, .. } => assert_eq!(*evidence_count, count),
            other => panic!("expected reused, got {other:?}"),
        }
    }
    // Identical composed analysis, hence identical evidence ids.
    assert_eq!(first.analysis, second.analysis);
    assert_eq!(cache.len(), 3);
}

#[test]
fn cached_scene_run_is_used_instead_of_re_executing_the_producer() {
    let Some(clip) = three_scene_clip("cached_scene_run") else {
        return;
    };
    // A planted cache entry that differs from what detection would produce (2 scenes
    // instead of 3): if the operation reports it, the producer was not re-executed.
    let config = SceneConfig::default();
    let key = scene_key(&clip.content_hash, &config);
    let planted = SceneRun {
        receipt: media_intelligence_core::ProducerReceipt {
            modality: Modality::Scene,
            producer: SCENE_PRODUCER.to_string(),
            producer_version: VISUAL_ANALYSIS_REVISION.to_string(),
            model: SCENE_MODEL.to_string(),
            model_version: SCENE_MODEL_VERSION.to_string(),
            config_hash: config.config_hash(),
            input_hash: expected_input_hash(&clip.content_hash),
            run_id: Some("planted-by-acceptance-test".to_string()),
            config: None,
        },
        scenes: vec![
            SceneInterval {
                id: "planted-0".into(),
                index: 0,
                start_seconds: 0.0,
                end_seconds: 8.0,
                start_frame: Some(0),
                end_frame: Some(200),
            },
            SceneInterval {
                id: "planted-1".into(),
                index: 1,
                start_seconds: 8.0,
                end_seconds: 16.0,
                start_frame: Some(200),
                end_frame: Some(400),
            },
        ],
    };
    let mut cache = InMemoryStageCache::new();
    cache.put(key, CachedStage::Scenes(planted.clone()));

    let report = analyze(
        &request(&clip, scenes_only()),
        &Producers::default(),
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    match &report.scenes {
        StageStatus::Reused {
            receipt,
            evidence_count,
        } => {
            assert_eq!(*evidence_count, 2);
            assert_eq!(receipt, &planted.receipt);
        }
        other => panic!("expected reused, got {other:?}"),
    }
    assert_eq!(scene_run_from_evidence(&report.analysis), planted);
}

#[test]
fn changing_scene_config_re_executes_only_scenes() {
    let Some(clip) = three_scene_clip("scene_config_change") else {
        return;
    };
    let speech = FakeSpeech::new(Behaviour::Succeed);
    let ocr = FakeOcr::new(Behaviour::Succeed);
    let producers = Producers {
        speech: Some(&speech),
        visible_text: Some(&ocr),
    };
    let mut cache = InMemoryStageCache::new();
    let first = analyze(
        &request(&clip, everything()),
        &producers,
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();

    let changed = SceneConfig {
        threshold: 27.0,
        min_scene_len: 200,
    };
    let mut profile = everything();
    profile.scenes = Some(changed);
    let second = analyze(
        &request(&clip, profile),
        &producers,
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();

    // Scenes really re-executed: the new configuration yields one scene.
    match &second.scenes {
        StageStatus::CompletedWithFindings {
            receipt,
            evidence_count,
        } => {
            assert_eq!(*evidence_count, 1);
            assert_eq!(receipt.config_hash, changed.config_hash());
        }
        other => panic!("scenes must re-execute, got {other:?}"),
    }
    assert!(matches!(second.speech, StageStatus::Reused { .. }));
    assert!(matches!(second.visible_text, StageStatus::Reused { .. }));
    assert_eq!(speech.calls.get(), 1);
    assert_eq!(ocr.calls.get(), 1);

    // Unchanged modalities keep their evidence ids; scene ids change.
    for modality in [Modality::Speech, Modality::VisibleText] {
        assert_eq!(
            evidence_ids(&first.analysis, modality),
            evidence_ids(&second.analysis, modality)
        );
    }
    let old_scenes = evidence_ids(&first.analysis, Modality::Scene);
    for id in evidence_ids(&second.analysis, Modality::Scene) {
        assert!(!old_scenes.contains(&id));
    }
    assert_eq!(cache.len(), 4, "both scene configurations are cached");
}

#[test]
fn changing_a_provider_configuration_re_executes_only_that_provider() {
    let Some(clip) = three_scene_clip("provider_config_change") else {
        return;
    };
    let speech_a = FakeSpeech::with_config(Behaviour::Succeed, "sha256:speech-config-a");
    let speech_b = FakeSpeech::with_config(Behaviour::Succeed, "sha256:speech-config-b");
    let ocr = FakeOcr::new(Behaviour::Succeed);
    let mut cache = InMemoryStageCache::new();
    let first = analyze(
        &request(&clip, everything()),
        &Producers {
            speech: Some(&speech_a),
            visible_text: Some(&ocr),
        },
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    let second = analyze(
        &request(&clip, everything()),
        &Producers {
            speech: Some(&speech_b),
            visible_text: Some(&ocr),
        },
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();

    assert_eq!(speech_b.calls.get(), 1, "new speech configuration executes");
    assert!(matches!(
        second.speech,
        StageStatus::CompletedWithFindings { ref receipt, .. }
            if receipt.config_hash == "sha256:speech-config-b"
    ));
    assert!(matches!(second.visible_text, StageStatus::Reused { .. }));
    assert!(matches!(second.scenes, StageStatus::Reused { .. }));
    assert_eq!(ocr.calls.get(), 1);
    assert_eq!(
        evidence_ids(&first.analysis, Modality::Scene),
        evidence_ids(&second.analysis, Modality::Scene)
    );
    assert_ne!(
        evidence_ids(&first.analysis, Modality::Speech),
        evidence_ids(&second.analysis, Modality::Speech)
    );
}

#[test]
fn different_source_bytes_are_never_served_from_cache() {
    let Some(first_clip) = three_scene_clip("different_bytes_a") else {
        return;
    };
    let Some(second_clip) = generate_clip("different_bytes_b", &[("black", 6), ("white", 6)])
    else {
        return;
    };
    assert_ne!(first_clip.content_hash, second_clip.content_hash);
    let speech = FakeSpeech::new(Behaviour::Succeed);
    let producers = Producers {
        speech: Some(&speech),
        visible_text: None,
    };
    let mut profile = scenes_only();
    profile.speech = true;
    let mut cache = InMemoryStageCache::new();
    analyze(
        &request(&first_clip, profile.clone()),
        &producers,
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    let second = analyze(
        &request(&second_clip, profile),
        &producers,
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(speech.calls.get(), 2);
    assert!(matches!(
        second.speech,
        StageStatus::CompletedWithFindings { .. }
    ));
    match &second.scenes {
        StageStatus::CompletedWithFindings {
            receipt,
            evidence_count,
        } => {
            assert_eq!(*evidence_count, 2, "black/white clip has two scenes");
            assert_eq!(
                receipt.input_hash,
                expected_input_hash(&second_clip.content_hash)
            );
        }
        other => panic!("{other:?}"),
    }
}
