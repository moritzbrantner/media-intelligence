//! Repeated OCR frames collapse only through producer-supplied track membership.

mod common;

use common::{item, label, lecture};
use media_intelligence_core::{compose, EvidenceContent, Modality, ModalityInput};

fn visible_text(
    analysis: &media_intelligence_core::EvidenceAnalysis,
) -> Vec<&media_intelligence_core::VisibleTextEvidence> {
    analysis
        .evidence
        .iter()
        .filter_map(|e| match &e.content {
            EvidenceContent::VisibleText(text) => Some(text),
            _ => None,
        })
        .collect()
}

#[test]
fn a_track_of_repeated_frames_is_one_item_linked_to_all_observations() {
    let analysis = compose(&lecture()).unwrap();

    // 8 observations: 4 in trk-title, 2 in trk-slide, 2 untracked → 4 items.
    assert_eq!(
        analysis
            .evidence
            .iter()
            .filter(|e| e.modality == Modality::VisibleText)
            .count(),
        4
    );

    let EvidenceContent::VisibleText(title) = &item(&analysis, "track:trk-title").content else {
        panic!("visible text")
    };
    let ids: Vec<_> = title
        .observations
        .iter()
        .map(|o| o.observation_id.as_str())
        .collect();
    assert_eq!(
        ids,
        ["ocr-title-1", "ocr-title-2", "ocr-title-3", "ocr-title-4"]
    );
    let times: Vec<_> = title
        .observations
        .iter()
        .map(|o| o.timestamp_seconds)
        .collect();
    assert_eq!(times, [2.0, 4.0, 6.0, 8.0]);
    assert_eq!(title.text, "Rust Ownership");

    // No member observation surfaces as its own item.
    for text in visible_text(&analysis) {
        if text.track_id.as_deref() != Some("trk-title") {
            assert!(text
                .observations
                .iter()
                .all(|o| !o.observation_id.starts_with("ocr-title-")));
        }
    }
}

#[test]
fn untracked_observations_are_never_merged_with_each_other() {
    let analysis = compose(&lecture()).unwrap();

    // Same text, not in any track: two separate single-observation items.
    let main: Vec<_> = visible_text(&analysis)
        .into_iter()
        .filter(|text| text.text == "fn main()")
        .collect();
    assert_eq!(main.len(), 2);
    for text in main {
        assert_eq!(text.track_id, None);
        assert_eq!(text.observations.len(), 1);
    }
    assert_eq!(item(&analysis, "obs:ocr-main-2").start_seconds, 25.0);
}

#[test]
fn the_core_does_not_track_on_its_own() {
    // Drop producer track membership: identical text at consecutive frames must now
    // stay one item per observation (OCR tracking is owned by visual-analysis).
    let mut input = lecture();
    let ModalityInput::Completed { runs } = &mut input.visible_text else {
        panic!("fixture has completed OCR")
    };
    runs[0].tracks.clear();

    let analysis = compose(&input).unwrap();
    let texts = visible_text(&analysis);
    assert_eq!(texts.len(), 8);
    for text in texts {
        assert_eq!(text.track_id, None);
        assert_eq!(text.observations.len(), 1);
    }
    assert!(analysis
        .evidence
        .iter()
        .any(|e| label(e) == "obs:ocr-title-3"));
}
