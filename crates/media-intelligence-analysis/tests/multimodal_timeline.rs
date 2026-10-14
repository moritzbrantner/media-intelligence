//! Caller-supplied (fake) speech and OCR runs composed with real scenes on one timeline.

mod common;

use std::collections::BTreeSet;

use common::*;
use media_intelligence_analysis::{
    analyze, CancellationToken, InMemoryStageCache, Modality, Producers, StageStatus,
};
use media_intelligence_core::{
    compose, CompositionInput, ModalityInput, RelationKind, SpeechRun, VisibleTextRun,
};

#[test]
fn fake_speech_and_ocr_share_one_timeline_with_real_scenes() {
    let Some(clip) = three_scene_clip("multimodal_timeline") else {
        return;
    };
    let speech = FakeSpeech::new(Behaviour::Succeed);
    let ocr = FakeOcr::new(Behaviour::Succeed);
    let request = request(&clip, everything());
    let report = analyze(
        &request,
        &Producers {
            speech: Some(&speech),
            visible_text: Some(&ocr),
        },
        &mut InMemoryStageCache::new(),
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(speech.calls.get(), 1);
    assert_eq!(ocr.calls.get(), 1);

    assert!(matches!(
        report.speech,
        StageStatus::CompletedWithFindings {
            evidence_count: 4,
            ..
        }
    ));
    // 4 OCR frames, 2 of them in one producer track -> 3 items.
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

    // Every item is grounded in the same media identity.
    for item in &report.analysis.evidence {
        assert_eq!(item.content_hash, clip.content_hash);
        assert_eq!(item.asset_id, request.media.asset_id);
        assert_eq!(
            item.receipt.input_hash,
            expected_input_hash(&clip.content_hash)
        );
    }

    let relation =
        |kind, subject: &str, object: &str| (kind, subject.to_string(), object.to_string());
    let expected: BTreeSet<_> = [
        relation(RelationKind::SceneSpeech, "scene#0", "speech#0"),
        relation(RelationKind::SceneSpeech, "scene#0", "speech#1"),
        relation(RelationKind::SceneSpeech, "scene#1", "speech#1"),
        relation(RelationKind::SceneSpeech, "scene#1", "speech#2"),
        relation(RelationKind::SceneSpeech, "scene#2", "speech#3"),
        relation(RelationKind::SceneVisibleText, "scene#0", "ocr:o1"),
        relation(RelationKind::SceneVisibleText, "scene#1", "ocr:t1"),
        relation(RelationKind::SceneVisibleText, "scene#2", "ocr:o4"),
        relation(
            RelationKind::SpeechVisibleTextCooccurrence,
            "speech#0",
            "ocr:o1",
        ),
        relation(
            RelationKind::SpeechVisibleTextCooccurrence,
            "speech#2",
            "ocr:t1",
        ),
        relation(
            RelationKind::SpeechVisibleTextCooccurrence,
            "speech#3",
            "ocr:o4",
        ),
    ]
    .into_iter()
    .collect();
    assert_eq!(labelled_relations(&report.analysis), expected);

    // Exactly what the core composes from the producers' runs plus the real scene run.
    let speech_run: SpeechRun = {
        let identity = speech.identity.clone();
        SpeechRun {
            receipt: media_intelligence_core::ProducerReceipt {
                modality: Modality::Speech,
                producer: identity.producer,
                producer_version: identity.producer_version,
                model: identity.model,
                model_version: identity.model_version,
                config_hash: identity.config_hash,
                input_hash: expected_input_hash(&clip.content_hash),
                run_id: None,
                config: None,
            },
            segments: fake_segments(),
        }
    };
    let ocr_run: VisibleTextRun = {
        let identity = ocr.identity.clone();
        fake_ocr(
            media_intelligence_core::ProducerReceipt {
                modality: Modality::VisibleText,
                producer: identity.producer,
                producer_version: identity.producer_version,
                model: identity.model,
                model_version: identity.model_version,
                config_hash: identity.config_hash,
                input_hash: expected_input_hash(&clip.content_hash),
                run_id: None,
                config: None,
            },
            false,
        )
    };
    let recomposed = compose(&CompositionInput {
        media: request.media.clone(),
        speech: ModalityInput::Completed {
            runs: vec![speech_run],
        },
        visible_text: ModalityInput::Completed {
            runs: vec![ocr_run],
        },
        scenes: ModalityInput::Completed {
            runs: vec![scene_run_from_evidence(&report.analysis)],
        },
    })
    .unwrap();
    assert_eq!(report.analysis, recomposed);
}
