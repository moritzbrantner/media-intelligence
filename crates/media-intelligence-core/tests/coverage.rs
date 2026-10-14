//! Coverage distinguishes not-requested, unprocessed, failed, completed-empty and
//! completed-with-findings; a missing modality never reads as completed-empty.

mod common;

use common::{coverage, lecture, load_input};
use media_intelligence_core::{compose, CompositionError, CoverageStatus, Modality, ModalityInput};

#[test]
fn completed_modalities_report_findings_with_their_receipts() {
    let input = lecture();
    let analysis = compose(&input).unwrap();

    let modalities: Vec<_> = analysis.coverage.iter().map(|c| c.modality).collect();
    assert_eq!(
        modalities,
        [Modality::Speech, Modality::VisibleText, Modality::Scene]
    );

    let ModalityInput::Completed { runs } = &input.speech else {
        unreachable!()
    };
    assert_eq!(
        coverage(&analysis, Modality::Speech),
        &CoverageStatus::CompletedWithFindings {
            receipts: vec![runs[0].receipt.clone()],
            evidence_count: 6,
        }
    );
    assert!(matches!(
        coverage(&analysis, Modality::VisibleText),
        CoverageStatus::CompletedWithFindings {
            evidence_count: 4,
            ..
        }
    ));
    assert!(matches!(
        coverage(&analysis, Modality::Scene),
        CoverageStatus::CompletedWithFindings {
            evidence_count: 3,
            ..
        }
    ));
}

#[test]
fn failed_unprocessed_and_completed_empty_are_distinct() {
    let input = load_input("partial_coverage.json");
    let analysis = compose(&input).unwrap();

    let ModalityInput::Failed { receipt, .. } = &input.speech else {
        unreachable!()
    };
    assert_eq!(
        coverage(&analysis, Modality::Speech),
        &CoverageStatus::Failed {
            receipt: receipt.clone(),
            reason: "audio decoder error: no audio stream in container".to_string(),
        }
    );
    assert_eq!(
        coverage(&analysis, Modality::VisibleText),
        &CoverageStatus::Unprocessed
    );

    let ModalityInput::Completed { runs } = &input.scenes else {
        unreachable!()
    };
    assert_eq!(
        coverage(&analysis, Modality::Scene),
        &CoverageStatus::CompletedEmpty {
            receipts: vec![runs[0].receipt.clone()]
        }
    );

    assert!(analysis.evidence.is_empty());
    assert!(analysis.relations.is_empty());
}

#[test]
fn not_requested_is_distinct_from_unprocessed() {
    let mut input = load_input("partial_coverage.json");
    input.speech = ModalityInput::NotRequested;
    let analysis = compose(&input).unwrap();
    assert_eq!(
        coverage(&analysis, Modality::Speech),
        &CoverageStatus::NotRequested
    );
    assert_eq!(
        coverage(&analysis, Modality::VisibleText),
        &CoverageStatus::Unprocessed
    );
}

#[test]
fn all_five_states_serialize_distinguishably() {
    let mut input = load_input("partial_coverage.json");
    let failed = compose(&input).unwrap();
    input.speech = ModalityInput::NotRequested;
    let not_requested = compose(&input).unwrap();
    let findings = compose(&lecture()).unwrap();

    let states: std::collections::BTreeSet<String> = [
        &failed.coverage,
        &not_requested.coverage,
        &findings.coverage,
    ]
    .into_iter()
    .flatten()
    .map(|c| {
        serde_json::to_value(&c.status).unwrap()["state"]
            .as_str()
            .unwrap()
            .to_string()
    })
    .collect();
    assert_eq!(states.len(), 5, "five distinct coverage states: {states:?}");
}

#[test]
fn missing_modalities_never_read_as_completed_empty() {
    let mut input = lecture();
    input.speech = ModalityInput::Unprocessed;
    input.scenes = ModalityInput::NotRequested;
    let analysis = compose(&input).unwrap();

    for modality in [Modality::Speech, Modality::Scene] {
        assert!(!matches!(
            coverage(&analysis, modality),
            CoverageStatus::CompletedEmpty { .. } | CoverageStatus::CompletedWithFindings { .. }
        ));
        assert!(analysis.evidence.iter().all(|e| e.modality != modality));
    }
    assert!(matches!(
        coverage(&analysis, Modality::VisibleText),
        CoverageStatus::CompletedWithFindings { .. }
    ));
    assert!(
        analysis.relations.is_empty(),
        "no scenes and no speech → nothing to relate"
    );
}

#[test]
fn completed_without_any_producer_run_is_rejected() {
    // "Completed" with no receipt would be indistinguishable from a missing modality.
    let mut input = lecture();
    input.speech = ModalityInput::Completed { runs: vec![] };
    assert!(matches!(
        compose(&input),
        Err(CompositionError::CompletedWithoutRun {
            modality: Modality::Speech
        })
    ));
}
