//! Deterministic output and per-modality invalidation of content-addressed ids.

mod common;

use std::collections::BTreeSet;

use common::{ids_of, labelled_relations, lecture, load_input, serialized, shuffle_input};
use media_intelligence_core::{compose, ContentHash, Modality, ModalityInput, RelationKind};

#[test]
fn identical_input_gives_byte_identical_output() {
    let input = lecture();
    assert_eq!(
        serialized(&compose(&input).unwrap()),
        serialized(&compose(&input).unwrap())
    );
}

#[test]
fn input_collection_order_does_not_matter() {
    for fixture in ["lecture_composition.json", "conflicting_ocr_revisions.json"] {
        let input = load_input(fixture);
        let mut shuffled = input.clone();
        shuffle_input(&mut shuffled);
        assert_ne!(input, shuffled, "{fixture}: permutation changed the input");
        assert_eq!(
            serialized(&compose(&input).unwrap()),
            serialized(&compose(&shuffled).unwrap()),
            "{fixture}"
        );
    }
}

#[test]
fn changing_ocr_config_hash_changes_only_ocr_evidence_ids() {
    let before_input = lecture();
    let mut after_input = before_input.clone();
    let ModalityInput::Completed { runs } = &mut after_input.visible_text else {
        unreachable!()
    };
    runs[0].receipt.config_hash = "fnv1a64:00000000deadbeef".to_string();

    let before = compose(&before_input).unwrap();
    let after = compose(&after_input).unwrap();

    assert_eq!(
        ids_of(&before, Modality::Speech),
        ids_of(&after, Modality::Speech)
    );
    assert_eq!(
        ids_of(&before, Modality::Scene),
        ids_of(&after, Modality::Scene)
    );
    let ocr_before = ids_of(&before, Modality::VisibleText);
    let ocr_after = ids_of(&after, Modality::VisibleText);
    assert_eq!(ocr_after.len(), ocr_before.len());
    assert!(
        ocr_before.is_disjoint(&ocr_after),
        "every OCR-derived id changes"
    );

    // Relations not touching OCR are byte-identical; OCR relations keep their shape
    // and only follow the new ids.
    let untouched = |analysis: &media_intelligence_core::EvidenceAnalysis| -> Vec<_> {
        analysis
            .relations
            .iter()
            .filter(|r| r.kind == RelationKind::SceneSpeech)
            .cloned()
            .collect()
    };
    assert_eq!(untouched(&before), untouched(&after));
    assert_eq!(labelled_relations(&before), labelled_relations(&after));
    for relation in &after.relations {
        if relation.kind != RelationKind::SceneSpeech {
            assert!(ocr_after.contains(&relation.object));
        }
    }
    assert_eq!(after.relations.len(), before.relations.len());
}

#[test]
fn changing_speech_model_version_changes_only_speech_evidence_ids() {
    let before_input = lecture();
    let mut after_input = before_input.clone();
    let ModalityInput::Completed { runs } = &mut after_input.speech else {
        unreachable!()
    };
    runs[0].receipt.model_version = "whisperx-3.8.7".to_string();

    let before = compose(&before_input).unwrap();
    let after = compose(&after_input).unwrap();
    assert!(ids_of(&before, Modality::Speech).is_disjoint(&ids_of(&after, Modality::Speech)));
    assert_eq!(
        ids_of(&before, Modality::Scene),
        ids_of(&after, Modality::Scene)
    );
    assert_eq!(
        ids_of(&before, Modality::VisibleText),
        ids_of(&after, Modality::VisibleText)
    );
    let ocr_scene = |analysis: &media_intelligence_core::EvidenceAnalysis| -> Vec<_> {
        analysis
            .relations
            .iter()
            .filter(|r| r.kind == RelationKind::SceneVisibleText)
            .cloned()
            .collect()
    };
    assert_eq!(ocr_scene(&before), ocr_scene(&after));
}

#[test]
fn evidence_ids_are_bound_to_the_media_content_revision() {
    let before_input = lecture();
    let mut after_input = before_input.clone();
    after_input.media.content_hash = ContentHash::from_bytes(b"lecture bytes v2");

    let before: BTreeSet<_> = compose(&before_input)
        .unwrap()
        .evidence
        .into_iter()
        .map(|e| e.id)
        .collect();
    let after: BTreeSet<_> = compose(&after_input)
        .unwrap()
        .evidence
        .into_iter()
        .map(|e| e.id)
        .collect();
    assert!(before.is_disjoint(&after));
}
