//! Real scene execution: synthetic ffmpeg clip, real visual-analysis producer run.

mod common;

use common::*;
use media_intelligence_analysis::{
    analyze, input_hash, CancellationToken, InMemoryStageCache, Modality, Producers, SceneConfig,
    StageStatus, SCENE_MODEL, SCENE_MODEL_VERSION, SCENE_PRODUCER, VISUAL_ANALYSIS_REVISION,
};
use media_intelligence_core::{
    compose, CompositionInput, CoverageStatus, EvidenceContent, ModalityInput,
};

#[test]
fn scene_receipt_constants_name_the_pinned_canonical_producer() {
    assert_eq!(SCENE_PRODUCER, "visual-analysis");
    assert_eq!(
        VISUAL_ANALYSIS_REVISION,
        "115c1f0caceb644993df3121b4625e674a9f1b7a"
    );
    assert_eq!(SCENE_MODEL, "scenedetect-core:content");
    assert_eq!(SCENE_MODEL_VERSION, "0.1.0");
}

#[test]
fn scene_config_defaults_and_hash_identity() {
    let default = SceneConfig::default();
    assert_eq!(default.threshold, 27.0);
    assert_eq!(default.min_scene_len, 15);

    let hash = default.config_hash();
    assert!(hash.starts_with("sha256:"), "{hash}");
    assert_eq!(hash.len(), "sha256:".len() + 64, "{hash}");
    assert_eq!(hash, SceneConfig::default().config_hash(), "deterministic");

    let threshold = SceneConfig {
        threshold: 30.0,
        ..default
    };
    let min_len = SceneConfig {
        min_scene_len: 200,
        ..default
    };
    assert_ne!(threshold.config_hash(), hash);
    assert_ne!(min_len.config_hash(), hash);
    assert_ne!(threshold.config_hash(), min_len.config_hash());
}

#[test]
fn input_hash_is_prefixed_content_hash() {
    let content = media_intelligence_analysis::ContentHash::from_bytes(b"abc");
    assert_eq!(input_hash(&content), expected_input_hash(&content));
}

#[test]
fn real_scene_run_yields_exact_boundaries_receipt_and_core_composition() {
    let Some(clip) = three_scene_clip("real_scene_run") else {
        return;
    };
    let request = request(&clip, scenes_only());
    let mut cache = InMemoryStageCache::new();
    let report = analyze(
        &request,
        &Producers::default(),
        &mut cache,
        &CancellationToken::new(),
    )
    .expect("scene analysis of an intact local clip succeeds");

    // Status: real execution in this operation, with three findings.
    let receipt = match &report.scenes {
        StageStatus::CompletedWithFindings {
            receipt,
            evidence_count,
        } => {
            assert_eq!(*evidence_count, 3);
            receipt.clone()
        }
        other => panic!("scenes must complete with findings, got {other:?}"),
    };
    assert_eq!(report.speech, StageStatus::NotRequested);
    assert_eq!(report.visible_text, StageStatus::NotRequested);
    assert_eq!(report.status(Modality::Scene), &report.scenes);

    // Receipt names the pinned canonical producer, config hash and input content hash.
    assert_eq!(receipt.modality, Modality::Scene);
    assert_eq!(receipt.producer, "visual-analysis");
    assert_eq!(
        receipt.producer_version,
        "115c1f0caceb644993df3121b4625e674a9f1b7a"
    );
    assert_eq!(receipt.model, "scenedetect-core:content");
    assert_eq!(receipt.model_version, "0.1.0");
    assert_eq!(receipt.config_hash, SceneConfig::default().config_hash());
    assert_eq!(receipt.input_hash, expected_input_hash(&clip.content_hash));
    assert_eq!(
        clip.content_hash.as_str(),
        sha256_hex(&std::fs::read(&clip.path).unwrap()),
        "content hash is the sha256 of the analysed bytes"
    );
    let recorded = receipt
        .config
        .as_ref()
        .expect("the effective configuration is recorded");
    assert_eq!(recorded["threshold"].as_f64(), Some(27.0), "{recorded}");
    assert_eq!(recorded["minSceneLen"].as_u64(), Some(15), "{recorded}");

    // Exact source-relative boundaries; visual-analysis frame indices carried verbatim
    // (0-based, end exclusive).
    let scenes: Vec<_> = report
        .analysis
        .evidence
        .iter()
        .filter(|item| item.modality == Modality::Scene)
        .collect();
    let expected = [
        (0, 0.0, 5.0, 0, 125),
        (1, 5.0, 12.0, 125, 300),
        (2, 12.0, 16.0, 300, 400),
    ];
    assert_eq!(scenes.len(), expected.len());
    for (item, (index, start, end, start_frame, end_frame)) in scenes.iter().zip(expected) {
        assert!((item.start_seconds - start).abs() < 1e-9, "{item:?}");
        assert!((item.end_seconds - end).abs() < 1e-9, "{item:?}");
        assert_eq!(item.content_hash, clip.content_hash);
        assert_eq!(item.asset_id, request.media.asset_id);
        assert_eq!(item.receipt, receipt);
        match &item.content {
            EvidenceContent::Scene(scene) => {
                assert_eq!(scene.scene_index, index);
                assert_eq!(scene.start_frame, Some(start_frame));
                assert_eq!(scene.end_frame, Some(end_frame));
            }
            other => panic!("expected scene content, got {other:?}"),
        }
    }

    // Coverage via the core.
    assert!(matches!(
        coverage(&report.analysis, Modality::Scene),
        CoverageStatus::CompletedWithFindings {
            evidence_count: 3,
            ..
        }
    ));
    assert_eq!(
        coverage(&report.analysis, Modality::Speech),
        &CoverageStatus::NotRequested
    );
    assert_eq!(
        coverage(&report.analysis, Modality::VisibleText),
        &CoverageStatus::NotRequested
    );

    // The analysis is exactly what the #25 core composes from this run.
    let recomposed = compose(&CompositionInput {
        media: request.media.clone(),
        speech: ModalityInput::NotRequested,
        visible_text: ModalityInput::NotRequested,
        scenes: ModalityInput::Completed {
            runs: vec![scene_run_from_evidence(&report.analysis)],
        },
    })
    .unwrap();
    assert_eq!(report.analysis, recomposed);
}

#[test]
fn scene_config_is_passed_to_the_real_producer() {
    let Some(clip) = three_scene_clip("scene_config_is_passed") else {
        return;
    };
    // A minimum scene length of 200 frames (8 s) merges the 4 s blue tail and the
    // 5 s red head away; visual-analysis then reports a single scene.
    let config = SceneConfig {
        threshold: 27.0,
        min_scene_len: 200,
    };
    let mut profile = scenes_only();
    profile.scenes = Some(config);
    let report = analyze(
        &request(&clip, profile),
        &Producers::default(),
        &mut InMemoryStageCache::new(),
        &CancellationToken::new(),
    )
    .unwrap();
    match &report.scenes {
        StageStatus::CompletedWithFindings {
            receipt,
            evidence_count,
        } => {
            assert_eq!(*evidence_count, 1);
            assert_eq!(receipt.config_hash, config.config_hash());
        }
        other => panic!("{other:?}"),
    }
    let scene = &report.analysis.evidence[0];
    assert!((scene.start_seconds - 0.0).abs() < 1e-9);
    assert!((scene.end_seconds - 16.0).abs() < 1e-9);
}
