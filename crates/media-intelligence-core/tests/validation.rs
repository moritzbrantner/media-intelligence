//! Invalid input returns errors instead of panicking or composing silently.

mod common;

use common::{lecture, load_input};
use media_intelligence_core::{
    compose, CompositionError, Modality, ModalityInput, ProducerReceipt, SpeechRun,
    TimedTextContract,
};

fn speech_runs(input: &mut media_intelligence_core::CompositionInput) -> &mut Vec<SpeechRun> {
    match &mut input.speech {
        ModalityInput::Completed { runs } => runs,
        _ => unreachable!(),
    }
}

#[test]
fn end_before_start_is_rejected() {
    let mut input = lecture();
    let segment = &mut speech_runs(&mut input)[0].segments[2];
    segment.end_seconds = segment.start_seconds - 0.5;
    assert!(matches!(
        compose(&input),
        Err(CompositionError::InvalidTimeRange {
            modality: Modality::Speech,
            ..
        })
    ));
}

#[test]
fn time_before_media_start_is_rejected() {
    let mut input = lecture();
    let ModalityInput::Completed { runs } = &mut input.scenes else {
        unreachable!()
    };
    runs[0].scenes[0].start_seconds = -1.0;
    assert!(matches!(
        compose(&input),
        Err(CompositionError::InvalidTimeRange {
            modality: Modality::Scene,
            ..
        })
    ));
}

#[test]
fn non_finite_times_are_rejected() {
    let mut input = lecture();
    let ModalityInput::Completed { runs } = &mut input.scenes else {
        unreachable!()
    };
    runs[0].scenes[1].end_seconds = f64::NAN;
    assert!(matches!(
        compose(&input),
        Err(CompositionError::NonFiniteTime {
            modality: Modality::Scene,
            ..
        })
    ));

    let mut input = lecture();
    let ModalityInput::Completed { runs } = &mut input.visible_text else {
        unreachable!()
    };
    runs[0].observations[4].timestamp_seconds = f64::INFINITY;
    assert!(matches!(
        compose(&input),
        Err(CompositionError::NonFiniteTime {
            modality: Modality::VisibleText,
            ..
        })
    ));
}

#[test]
fn track_referencing_an_unknown_observation_is_rejected() {
    let mut input = lecture();
    let ModalityInput::Completed { runs } = &mut input.visible_text else {
        unreachable!()
    };
    runs[0].tracks[0]
        .observation_ids
        .push("ocr-missing".to_string());
    assert_eq!(
        compose(&input).unwrap_err(),
        CompositionError::UnknownObservation {
            track_id: "trk-title".to_string(),
            observation_id: "ocr-missing".to_string(),
        }
    );
}

#[test]
fn track_membership_is_scoped_to_its_own_run() {
    let mut input = load_input("conflicting_ocr_revisions.json");
    let ModalityInput::Completed { runs } = &mut input.visible_text else {
        unreachable!()
    };
    let mut only_in_a = runs[0].observations[2].clone();
    only_in_a.id = "obs-only-in-run-a".to_string();
    runs[0].observations.push(only_in_a);
    runs[1].tracks[0]
        .observation_ids
        .push("obs-only-in-run-a".to_string());
    assert_eq!(
        compose(&input).unwrap_err(),
        CompositionError::UnknownObservation {
            track_id: "trk-1".to_string(),
            observation_id: "obs-only-in-run-a".to_string(),
        }
    );
}

#[test]
fn receipt_modality_must_match_its_slot() {
    let mut input = lecture();
    speech_runs(&mut input)[0].receipt.modality = Modality::VisibleText;
    assert_eq!(
        compose(&input).unwrap_err(),
        CompositionError::ReceiptModalityMismatch {
            expected: Modality::Speech,
            found: Modality::VisibleText,
        }
    );
}

#[test]
fn duplicate_producer_ids_within_one_run_are_rejected() {
    let mut input = lecture();
    let ModalityInput::Completed { runs } = &mut input.visible_text else {
        unreachable!()
    };
    let duplicate = runs[0].observations[0].clone();
    runs[0].observations.push(duplicate);
    assert!(matches!(
        compose(&input),
        Err(CompositionError::DuplicateId {
            modality: Modality::VisibleText,
            ..
        })
    ));
}

#[test]
fn untimed_timed_text_segments_are_rejected() {
    let transcript: TimedTextContract = serde_json::from_str(
        r#"{"segments":[{"index":0,"startSeconds":0.0,"endSeconds":1.0,"text":"timed","isFinal":true},
                        {"index":1,"text":"untimed","isFinal":true}]}"#,
    )
    .unwrap();
    let receipt: ProducerReceipt = serde_json::from_value(serde_json::json!({
        "modality": "speech", "producer": "audio-analysis", "producerVersion": "cc8075f6",
        "model": "whisper-base.en", "modelVersion": "ggml", "configHash": "sha256:00",
        "inputHash": "sha256:01"
    }))
    .unwrap();
    assert_eq!(
        SpeechRun::from_timed_text(receipt, &transcript).unwrap_err(),
        CompositionError::UntimedSegment { index: 1 }
    );
}
