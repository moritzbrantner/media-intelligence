//! Spoken and visible-text evidence of one video on one source-relative timeline.

mod common;

use common::{item, label, labels, lecture};
use media_intelligence_core::{
    compose, EvidenceContent, Modality, ModalityInput, EVIDENCE_ANALYSIS_SCHEMA,
    EVIDENCE_ANALYSIS_SCHEMA_VERSION,
};

#[test]
fn speech_scenes_and_visible_text_share_one_timeline_in_documented_order() {
    let analysis = compose(&lecture()).expect("lecture fixture composes");

    // Order: start, then end, then modality (speech < visible_text < scene), then id.
    assert_eq!(
        labels(&analysis),
        [
            "speech:0",        // [0, 3)
            "scene:sc-0",      // [0, 10)
            "track:trk-title", // [2, 8]
            "speech:1",        // [3, 9.5)
            "speech:2",        // [10, 12)
            "scene:sc-1",      // [10, 20)
            "obs:ocr-main-1",  // point 12
            "speech:3",        // [12, 16)
            "speech:4",        // [16, 18)
            "track:trk-slide", // [18, 22]
            "speech:5",        // [19, 26)
            "scene:sc-2",      // [20, 30)
            "obs:ocr-main-2",  // point 25
        ]
    );

    for pair in analysis.evidence.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let key_a = (a.start_seconds, a.end_seconds, a.modality);
        let key_b = (b.start_seconds, b.end_seconds, b.modality);
        assert!(
            key_a < key_b || (key_a == key_b && a.id < b.id),
            "{} must precede {}",
            label(a),
            label(b)
        );
    }
}

#[test]
fn exact_time_ranges_are_preserved() {
    let analysis = compose(&lecture()).unwrap();
    let expected = [
        ("speech:0", 0.0, 3.0),
        ("speech:1", 3.0, 9.5),
        ("speech:5", 19.0, 26.0),
        ("scene:sc-1", 10.0, 20.0),
        // A track spans its earliest to its latest member observation.
        ("track:trk-title", 2.0, 8.0),
        ("track:trk-slide", 18.0, 22.0),
        // An untracked observation is the instant it was sampled.
        ("obs:ocr-main-1", 12.0, 12.0),
    ];
    for (wanted, start, end) in expected {
        let found = item(&analysis, wanted);
        assert_eq!(
            (found.start_seconds, found.end_seconds),
            (start, end),
            "{wanted}"
        );
    }
}

#[test]
fn every_item_keeps_source_identity_and_its_own_producer_receipt() {
    let input = lecture();
    let analysis = compose(&input).unwrap();
    assert_eq!(analysis.schema, EVIDENCE_ANALYSIS_SCHEMA);
    assert_eq!(analysis.schema_version, EVIDENCE_ANALYSIS_SCHEMA_VERSION);
    assert_eq!(analysis.media, input.media);

    let receipt_of = |modality| match modality {
        Modality::Speech => match &input.speech {
            ModalityInput::Completed { runs } => runs[0].receipt.clone(),
            _ => unreachable!(),
        },
        Modality::VisibleText => match &input.visible_text {
            ModalityInput::Completed { runs } => runs[0].receipt.clone(),
            _ => unreachable!(),
        },
        Modality::Scene => match &input.scenes {
            ModalityInput::Completed { runs } => runs[0].receipt.clone(),
            _ => unreachable!(),
        },
    };

    for evidence in &analysis.evidence {
        assert_eq!(evidence.asset_id, input.media.asset_id);
        assert_eq!(evidence.content_hash, input.media.content_hash);
        assert_eq!(
            evidence.receipt,
            receipt_of(evidence.modality),
            "{}",
            label(evidence)
        );
        assert_eq!(evidence.receipt.modality, evidence.modality);
        let content_modality = match evidence.content {
            EvidenceContent::Speech(_) => Modality::Speech,
            EvidenceContent::VisibleText(_) => Modality::VisibleText,
            EvidenceContent::Scene(_) => Modality::Scene,
        };
        assert_eq!(content_modality, evidence.modality);
    }

    let ids: std::collections::BTreeSet<_> = analysis.evidence.iter().map(|e| &e.id).collect();
    assert_eq!(
        ids.len(),
        analysis.evidence.len(),
        "evidence ids are unique"
    );
}

#[test]
fn producer_content_is_carried_through_unchanged() {
    let analysis = compose(&lecture()).unwrap();

    let EvidenceContent::Speech(speech) = &item(&analysis, "speech:2").content else {
        panic!("speech content")
    };
    assert_eq!(speech.text, "Let's look at main.");
    assert_eq!(speech.language.as_deref(), Some("en"));
    assert_eq!(speech.speaker.as_deref(), Some("SPEAKER_00"));

    let EvidenceContent::Scene(scene) = &item(&analysis, "scene:sc-2").content else {
        panic!("scene content")
    };
    assert_eq!(scene.scene_index, 2);
    assert_eq!((scene.start_frame, scene.end_frame), (Some(600), Some(900)));

    let EvidenceContent::VisibleText(text) = &item(&analysis, "track:trk-slide").content else {
        panic!("visible text content")
    };
    assert_eq!(text.text, "Borrowing rules");
    assert_eq!(text.role.as_deref(), Some("presentation_slide"));
    assert_eq!(text.observations[0].frame_index, Some(540));
    assert_eq!(
        text.observations[0].region,
        Some(media_intelligence_core::Region {
            x: 96,
            y: 48,
            width: 512,
            height: 64
        })
    );
}

#[test]
fn confidence_meaning_stays_modality_specific() {
    let analysis = compose(&lecture()).unwrap();

    // Exact producer values: no cross-modal normalisation.
    let EvidenceContent::Speech(speech) = &item(&analysis, "speech:2").content else {
        panic!("speech content")
    };
    assert_eq!(speech.transcript_confidence, Some(0.42));

    let EvidenceContent::VisibleText(text) = &item(&analysis, "track:trk-title").content else {
        panic!("visible text content")
    };
    let ocr: Vec<_> = text.observations.iter().map(|o| o.ocr_confidence).collect();
    assert_eq!(ocr, [Some(0.97), Some(0.95), Some(0.96), Some(0.98)]);

    // No merged/shared score field anywhere on an item or its content.
    for evidence in &analysis.evidence {
        let value = serde_json::to_value(evidence).unwrap();
        for object in [&value, &value["content"]] {
            for key in object.as_object().unwrap().keys() {
                let lower = key.to_ascii_lowercase();
                assert!(
                    !matches!(
                        lower.as_str(),
                        "score" | "confidence" | "normalizedscore" | "relevance"
                    ),
                    "{} carries a modality-neutral score field {key}",
                    label(evidence)
                );
            }
        }
    }
}
