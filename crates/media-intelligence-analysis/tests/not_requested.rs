//! Modalities the profile does not select are never executed.

mod common;

use std::fs;

use common::*;
use media_intelligence_analysis::{
    analyze, AnalysisProfile, AnalysisRequest, CancellationToken, ContentHash, InMemoryStageCache,
    Modality, Producers, StageStatus,
};
use media_intelligence_core::CoverageStatus;

#[test]
fn supplied_producers_are_not_invoked_when_their_modality_is_not_requested() {
    let Some(clip) = three_scene_clip("producers_not_requested") else {
        return;
    };
    let speech = FakeSpeech::new(Behaviour::Succeed);
    let ocr = FakeOcr::new(Behaviour::Succeed);
    let mut cache = InMemoryStageCache::new();
    let report = analyze(
        &request(&clip, scenes_only()),
        &Producers {
            speech: Some(&speech),
            visible_text: Some(&ocr),
        },
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(speech.calls.get(), 0);
    assert_eq!(ocr.calls.get(), 0);
    assert_eq!(report.speech, StageStatus::NotRequested);
    assert_eq!(report.visible_text, StageStatus::NotRequested);
    assert_eq!(
        coverage(&report.analysis, Modality::Speech),
        &CoverageStatus::NotRequested
    );
    assert_eq!(
        coverage(&report.analysis, Modality::VisibleText),
        &CoverageStatus::NotRequested
    );
    assert!(matches!(
        report.scenes,
        StageStatus::CompletedWithFindings { .. }
    ));
    assert!(cache
        .keys()
        .iter()
        .all(|key| key.modality == Modality::Scene));
}

#[test]
fn scene_producer_is_not_invoked_when_scenes_are_not_requested() {
    // These bytes are not decodable video: had the scene producer run on them, the scene
    // stage would fail. No ffmpeg is needed.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("not-a-video.bin");
    fs::write(&path, b"plain bytes, not a video container").unwrap();
    let content_hash = ContentHash::from_bytes(fs::read(&path).unwrap());
    let speech = FakeSpeech::new(Behaviour::Succeed);
    let mut cache = InMemoryStageCache::new();
    let report = analyze(
        &AnalysisRequest {
            media: media_for(&path, content_hash),
            media_path: path.clone(),
            profile: AnalysisProfile {
                speech: true,
                visible_text: false,
                scenes: None,
            },
        },
        &Producers {
            speech: Some(&speech),
            visible_text: None,
        },
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(report.scenes, StageStatus::NotRequested);
    assert_eq!(
        coverage(&report.analysis, Modality::Scene),
        &CoverageStatus::NotRequested
    );
    assert_eq!(count(&report.analysis, Modality::Scene), 0);
    assert!(matches!(
        report.speech,
        StageStatus::CompletedWithFindings {
            evidence_count: 4,
            ..
        }
    ));
    assert_eq!(cache.len(), 1);
}

#[test]
fn empty_profile_runs_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("any.bin");
    fs::write(&path, b"x").unwrap();
    let speech = FakeSpeech::new(Behaviour::Succeed);
    let ocr = FakeOcr::new(Behaviour::Succeed);
    let mut cache = InMemoryStageCache::new();
    let report = analyze(
        &AnalysisRequest {
            media: media_for(&path, ContentHash::from_bytes(b"x")),
            media_path: path.clone(),
            profile: AnalysisProfile::default(),
        },
        &Producers {
            speech: Some(&speech),
            visible_text: Some(&ocr),
        },
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(speech.calls.get() + ocr.calls.get(), 0);
    assert_eq!(report.speech, StageStatus::NotRequested);
    assert_eq!(report.visible_text, StageStatus::NotRequested);
    assert_eq!(report.scenes, StageStatus::NotRequested);
    assert!(report.analysis.evidence.is_empty());
    assert!(cache.is_empty());
}
