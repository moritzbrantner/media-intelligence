//! Evidence from different producer revisions of one modality is never merged.

mod common;

use std::collections::BTreeSet;

use common::{coverage, load_input, revision_label};
use media_intelligence_core::{compose, CoverageStatus, EvidenceContent, Modality, ModalityInput};

#[test]
fn two_ocr_model_versions_on_one_video_stay_separate() {
    let input = load_input("conflicting_ocr_revisions.json");
    let ModalityInput::Completed { runs } = &input.visible_text else {
        unreachable!()
    };
    let analysis = compose(&input).unwrap();

    // Identical text/timestamps/ids in both runs: 2 items per run, nothing deduplicated.
    let labels: BTreeSet<_> = analysis.evidence.iter().map(revision_label).collect();
    let expected: BTreeSet<_> = [
        "track:trk-1@2024-03-01",
        "obs:obs-3@2024-03-01",
        "track:trk-1@2025-01-15",
        "obs:obs-3@2025-01-15",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(labels, expected);
    assert_eq!(analysis.evidence.len(), 4);

    let ids: BTreeSet<_> = analysis.evidence.iter().map(|e| &e.id).collect();
    assert_eq!(ids.len(), 4, "each revision gets its own evidence ids");

    // Each item keeps its own receipt and only its own run's observation scores.
    for evidence in &analysis.evidence {
        let run = runs
            .iter()
            .find(|run| run.receipt.model_version == evidence.receipt.model_version)
            .expect("receipt comes from an input run");
        assert_eq!(evidence.receipt, run.receipt);
        let EvidenceContent::VisibleText(text) = &evidence.content else {
            panic!()
        };
        for observation in &text.observations {
            let source = run
                .observations
                .iter()
                .find(|o| o.id == observation.observation_id)
                .unwrap();
            assert_eq!(observation.ocr_confidence, source.confidence);
        }
        if text.track_id.is_some() {
            assert_eq!(
                text.observations.len(),
                2,
                "track holds only its own run's frames"
            );
        }
    }

    let CoverageStatus::CompletedWithFindings {
        receipts,
        evidence_count,
    } = coverage(&analysis, Modality::VisibleText)
    else {
        panic!("visible text completed with findings")
    };
    assert_eq!(*evidence_count, 4);
    let got: BTreeSet<_> = receipts.iter().map(|r| r.model_version.clone()).collect();
    let want: BTreeSet<_> = runs
        .iter()
        .map(|r| r.receipt.model_version.clone())
        .collect();
    assert_eq!(got, want);
    assert_eq!(
        coverage(&analysis, Modality::Speech),
        &CoverageStatus::NotRequested
    );
    assert_eq!(
        coverage(&analysis, Modality::Scene),
        &CoverageStatus::NotRequested
    );
}
