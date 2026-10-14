//! Unavailable, failing, empty and mislabelled producers next to real scene execution.

mod common;

use common::*;
use media_intelligence_analysis::{
    analyze, CancellationToken, InMemoryStageCache, Modality, Producers, StageStatus,
};
use media_intelligence_core::CoverageStatus;

#[test]
fn requested_speech_and_ocr_without_producers_are_unavailable_while_scenes_complete() {
    let Some(clip) = three_scene_clip("unavailable_providers") else {
        return;
    };
    let report = analyze(
        &request(&clip, everything()),
        &Producers::default(),
        &mut InMemoryStageCache::new(),
        &CancellationToken::new(),
    )
    .unwrap();

    assert_eq!(report.speech, StageStatus::Unavailable);
    assert_eq!(report.visible_text, StageStatus::Unavailable);
    assert!(matches!(
        report.scenes,
        StageStatus::CompletedWithFindings {
            evidence_count: 3,
            ..
        }
    ));

    // Never reported as completed (empty or otherwise) in the composed analysis.
    for modality in [Modality::Speech, Modality::VisibleText] {
        assert_eq!(
            coverage(&report.analysis, modality),
            &CoverageStatus::Unprocessed,
            "{modality:?}"
        );
        assert_eq!(count(&report.analysis, modality), 0);
    }
    assert_eq!(count(&report.analysis, Modality::Scene), 3);
}

#[test]
fn failing_producer_is_failed_with_reason_while_other_modalities_succeed() {
    let Some(clip) = three_scene_clip("failing_provider") else {
        return;
    };
    let speech = FakeSpeech::new(Behaviour::Fail("speech model file not found".into()));
    let ocr = FakeOcr::new(Behaviour::Succeed);
    let producers = Producers {
        speech: Some(&speech),
        visible_text: Some(&ocr),
    };
    let mut cache = InMemoryStageCache::new();
    let report = analyze(
        &request(&clip, everything()),
        &producers,
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();

    match &report.speech {
        StageStatus::Failed { reason, .. } => {
            assert!(reason.contains("speech model file not found"), "{reason}")
        }
        other => panic!("speech must be failed, got {other:?}"),
    }
    assert!(matches!(
        coverage(&report.analysis, Modality::Speech),
        CoverageStatus::Failed { reason, .. } if reason.contains("speech model file not found")
    ));
    assert_eq!(count(&report.analysis, Modality::Speech), 0);

    assert!(matches!(
        report.visible_text,
        StageStatus::CompletedWithFindings {
            evidence_count: 3,
            ..
        }
    ));
    assert!(matches!(
        report.scenes,
        StageStatus::CompletedWithFindings {
            evidence_count: 3,
            ..
        }
    ));
    assert_eq!(count(&report.analysis, Modality::VisibleText), 3);
    assert_eq!(count(&report.analysis, Modality::Scene), 3);

    // Failures are not cached: a rerun invokes the failing producer again, while the
    // successful stages are reused.
    let rerun = analyze(
        &request(&clip, everything()),
        &producers,
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(speech.calls.get(), 2);
    assert!(matches!(rerun.speech, StageStatus::Failed { .. }));
    assert_eq!(ocr.calls.get(), 1);
    assert!(matches!(rerun.visible_text, StageStatus::Reused { .. }));
    assert!(matches!(rerun.scenes, StageStatus::Reused { .. }));
}

#[test]
fn completed_empty_is_distinct_from_unavailable() {
    let Some(clip) = three_scene_clip("completed_empty") else {
        return;
    };
    let speech = FakeSpeech::new(Behaviour::SucceedEmpty);
    let report = analyze(
        &request(&clip, everything()),
        &Producers {
            speech: Some(&speech),
            visible_text: None,
        },
        &mut InMemoryStageCache::new(),
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(report.speech, StageStatus::CompletedEmpty { .. }));
    assert!(matches!(
        coverage(&report.analysis, Modality::Speech),
        CoverageStatus::CompletedEmpty { .. }
    ));
    assert_eq!(report.visible_text, StageStatus::Unavailable);
    assert_eq!(
        coverage(&report.analysis, Modality::VisibleText),
        &CoverageStatus::Unprocessed
    );
}

#[test]
fn run_with_a_foreign_input_hash_is_rejected_not_composed() {
    let Some(clip) = three_scene_clip("foreign_input_hash") else {
        return;
    };
    let speech = FakeSpeech::new(Behaviour::WrongInputHash);
    let ocr = FakeOcr::new(Behaviour::Succeed);
    let report = analyze(
        &request(&clip, everything()),
        &Producers {
            speech: Some(&speech),
            visible_text: Some(&ocr),
        },
        &mut InMemoryStageCache::new(),
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(
        matches!(report.speech, StageStatus::Failed { .. }),
        "a run that names other input bytes must not be labelled as evidence for this media: {:?}",
        report.speech
    );
    assert_eq!(count(&report.analysis, Modality::Speech), 0);
    assert!(matches!(
        report.visible_text,
        StageStatus::CompletedWithFindings { .. }
    ));
    assert!(matches!(
        report.scenes,
        StageStatus::CompletedWithFindings { .. }
    ));
}
