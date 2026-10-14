//! Missing or changed media is an error or failure, never mislabelled evidence.

mod common;

use std::fs;

use common::*;
use media_intelligence_analysis::{
    analyze, AnalysisError, AnalysisRequest, CancellationToken, ContentHash, InMemoryStageCache,
    Modality, Producers, SceneConfig, StageStatus,
};

#[test]
fn missing_media_file_is_an_error_and_runs_no_producer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("does-not-exist.mp4");
    let speech = FakeSpeech::new(Behaviour::Succeed);
    let ocr = FakeOcr::new(Behaviour::Succeed);
    let mut cache = InMemoryStageCache::new();
    let result = analyze(
        &AnalysisRequest {
            media: media_for(&path, ContentHash::from_bytes(b"anything")),
            media_path: path.clone(),
            profile: everything(),
        },
        &Producers {
            speech: Some(&speech),
            visible_text: Some(&ocr),
        },
        &mut cache,
        &CancellationToken::new(),
    );
    match result {
        Err(AnalysisError::MediaMissing { path: reported }) => assert_eq!(reported, path),
        other => panic!("expected MediaMissing, got {other:?}"),
    }
    assert_eq!(speech.calls.get(), 0);
    assert_eq!(ocr.calls.get(), 0);
    assert!(cache.is_empty());
}

#[test]
fn bytes_that_do_not_match_the_declared_content_hash_are_rejected() {
    let Some(clip) = three_scene_clip("declared_hash_mismatch") else {
        return;
    };
    let declared = clip.content_hash.clone();
    // The caller's identity describes the original bytes; the file was replaced since.
    let mut bytes = fs::read(&clip.path).unwrap();
    bytes.extend_from_slice(b"appended");
    fs::write(&clip.path, &bytes).unwrap();
    let actual = ContentHash::from_bytes(&bytes);

    let speech = FakeSpeech::new(Behaviour::Succeed);
    let mut cache = InMemoryStageCache::new();
    let result = analyze(
        &AnalysisRequest {
            media: media_for(&clip.path, declared.clone()),
            media_path: clip.path.clone(),
            profile: everything(),
        },
        &Producers {
            speech: Some(&speech),
            visible_text: None,
        },
        &mut cache,
        &CancellationToken::new(),
    );
    match result {
        Err(AnalysisError::ContentHashMismatch {
            declared: reported_declared,
            actual: reported_actual,
        }) => {
            assert_eq!(reported_declared, declared);
            assert_eq!(reported_actual, actual);
        }
        other => panic!("expected ContentHashMismatch, got {other:?}"),
    }
    assert_eq!(speech.calls.get(), 0);
    assert!(cache.is_empty());
}

#[test]
fn bytes_changed_during_the_operation_never_complete_scenes_for_the_declared_hash() {
    let Some(clip) = three_scene_clip("bytes_changed_during_run") else {
        return;
    };
    // Speech runs first and replaces the media file; the scene stage must not report a
    // completed (or reused) run for the declared content hash afterwards.
    let speech = FakeSpeech::new(Behaviour::RewriteMediaThenSucceed);
    let mut cache = InMemoryStageCache::new();
    let mut profile = scenes_only();
    profile.speech = true;
    let result = analyze(
        &request(&clip, profile),
        &Producers {
            speech: Some(&speech),
            visible_text: None,
        },
        &mut cache,
        &CancellationToken::new(),
    );
    match result {
        Err(_) => {}
        Ok(report) => {
            assert!(
                matches!(report.scenes, StageStatus::Failed { .. }),
                "scenes must fail, got {:?}",
                report.scenes
            );
            assert_eq!(count(&report.analysis, Modality::Scene), 0);
        }
    }
    assert!(
        !cache
            .keys()
            .iter()
            .any(|key| key.modality == Modality::Scene),
        "no scene run is cached under the declared content hash"
    );
}

#[test]
fn invalid_scene_configuration_is_rejected_before_any_stage_runs() {
    let Some(clip) = three_scene_clip("invalid_scene_config") else {
        return;
    };
    for config in [
        SceneConfig {
            threshold: f32::NAN,
            min_scene_len: 15,
        },
        SceneConfig {
            threshold: -1.0,
            min_scene_len: 15,
        },
        SceneConfig {
            threshold: 27.0,
            min_scene_len: 0,
        },
    ] {
        let speech = FakeSpeech::new(Behaviour::Succeed);
        let mut profile = everything();
        profile.scenes = Some(config);
        let result = analyze(
            &request(&clip, profile),
            &Producers {
                speech: Some(&speech),
                visible_text: None,
            },
            &mut InMemoryStageCache::new(),
            &CancellationToken::new(),
        );
        assert!(
            matches!(result, Err(AnalysisError::InvalidSceneConfig { .. })),
            "{config:?}: {result:?}"
        );
        assert_eq!(speech.calls.get(), 0);
    }
}
