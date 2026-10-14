//! Cancellation stops new work and fabricates no results.

mod common;

use common::*;
use media_intelligence_analysis::{
    analyze, CancellationToken, InMemoryStageCache, Modality, Producers, StageStatus,
};
use media_intelligence_core::CoverageStatus;

#[test]
fn token_starts_uncancelled_and_clones_share_state() {
    let token = CancellationToken::new();
    assert!(!token.is_cancelled());
    let clone = token.clone();
    clone.cancel();
    assert!(token.is_cancelled());
}

#[test]
fn cancelled_before_any_stage_runs_nothing_and_reports_cancelled() {
    let Some(clip) = three_scene_clip("cancel_before") else {
        return;
    };
    let speech = FakeSpeech::new(Behaviour::Succeed);
    let ocr = FakeOcr::new(Behaviour::Succeed);
    let token = CancellationToken::new();
    token.cancel();
    let mut cache = InMemoryStageCache::new();
    let report = analyze(
        &request(&clip, everything()),
        &Producers {
            speech: Some(&speech),
            visible_text: Some(&ocr),
        },
        &mut cache,
        &token,
    )
    .unwrap();

    assert_eq!(speech.calls.get(), 0);
    assert_eq!(ocr.calls.get(), 0);
    assert_eq!(report.speech, StageStatus::Cancelled);
    assert_eq!(report.visible_text, StageStatus::Cancelled);
    assert_eq!(report.scenes, StageStatus::Cancelled);
    assert!(report.analysis.evidence.is_empty());
    assert!(report.analysis.relations.is_empty());
    for modality in [Modality::Speech, Modality::VisibleText, Modality::Scene] {
        assert_eq!(
            coverage(&report.analysis, modality),
            &CoverageStatus::Unprocessed,
            "{modality:?}"
        );
    }
    assert!(cache.is_empty(), "nothing completed, nothing cached");
}

#[test]
fn cancellation_during_a_stage_stops_later_stages() {
    let Some(clip) = three_scene_clip("cancel_during") else {
        return;
    };
    // Stages run in modality order; speech cancels the shared token and stops.
    let speech = FakeSpeech::new(Behaviour::CancelThenStop);
    let ocr = FakeOcr::new(Behaviour::Succeed);
    let token = CancellationToken::new();
    let mut cache = InMemoryStageCache::new();
    let report = analyze(
        &request(&clip, everything()),
        &Producers {
            speech: Some(&speech),
            visible_text: Some(&ocr),
        },
        &mut cache,
        &token,
    )
    .unwrap();

    assert!(token.is_cancelled());
    assert_eq!(speech.calls.get(), 1);
    assert_eq!(
        report.speech,
        StageStatus::Cancelled,
        "not failed, not completed"
    );
    assert_eq!(ocr.calls.get(), 0, "no new stage starts after cancellation");
    assert_eq!(report.visible_text, StageStatus::Cancelled);
    assert_eq!(report.scenes, StageStatus::Cancelled);
    assert!(report.analysis.evidence.is_empty());
    assert!(cache.is_empty());
}

#[test]
fn not_requested_stays_not_requested_under_cancellation() {
    let Some(clip) = three_scene_clip("cancel_not_requested") else {
        return;
    };
    let token = CancellationToken::new();
    token.cancel();
    let report = analyze(
        &request(&clip, scenes_only()),
        &Producers::default(),
        &mut InMemoryStageCache::new(),
        &token,
    )
    .unwrap();
    assert_eq!(report.speech, StageStatus::NotRequested);
    assert_eq!(report.visible_text, StageStatus::NotRequested);
    assert_eq!(report.scenes, StageStatus::Cancelled);
}
