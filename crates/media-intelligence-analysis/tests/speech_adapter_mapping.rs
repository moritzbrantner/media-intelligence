//! #32: mapping a recorded audio-analysis transcription response into a `SpeechRun`.
//!
//! The response JSON is written by the acceptance agent in the upstream serde shape of
//! `TranscriptionPipelineResponse` (audio-analysis cc8075f6) with a Foundation
//! `TimedTextContract` transcript. It is not model output.

#![cfg(feature = "audio-analysis")]

mod speech_common;

use media_intelligence_analysis::speech::{
    speech_run_from_response, TranscriptionPipelineResponse,
};
use media_intelligence_analysis::{Modality, ProducerError, ProducerIdentity, SpeechRun};
use media_intelligence_core::TranscriptSegment;
use serde_json::{json, Value};
use speech_common::EXPECTED_AUDIO_ANALYSIS_REVISION;

/// Fictitious input hash; the mapping only carries it into the receipt.
const INPUT_HASH: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";

fn identity() -> ProducerIdentity {
    ProducerIdentity {
        producer: "audio-analysis:candle-whisper".to_string(),
        producer_version: EXPECTED_AUDIO_ANALYSIS_REVISION.to_string(),
        model: "openai/whisper-tiny".to_string(),
        model_version: "unspecified".to_string(),
        config_hash: "sha256:1111111111111111111111111111111111111111111111111111111111111111"
            .to_string(),
    }
}

/// Confidences are exactly representable in f32 so the f32 -> f64 widening is exact.
fn recorded() -> Value {
    json!({
        "accepted": true,
        "operation": "transcribe",
        "provider": "candle-whisper",
        "modelId": "openai/whisper-tiny",
        "transcript": {
            "text": "hello there general kenobi no confidence",
            "language": "en",
            "segments": [
                {
                    "index": 0,
                    "startSeconds": 0.5,
                    "endSeconds": 2.25,
                    "text": "hello there",
                    "confidence": 0.875,
                    "isFinal": true
                },
                {
                    "index": 1,
                    "startSeconds": 2.5,
                    "endSeconds": 4.0,
                    "text": "general kenobi",
                    "language": "de",
                    "speaker": "SPEAKER_00",
                    "confidence": 0.5,
                    "isFinal": true
                },
                {
                    "index": 2,
                    "startSeconds": 5.0,
                    "endSeconds": 6.125,
                    "text": "no confidence",
                    "isFinal": true
                }
            ],
            "source": "clip.mp4"
        },
        "vadSegments": [],
        "alignment": null,
        "diarization": null,
        "artifacts": [],
        "diagnostics": ["recorded by the #32 acceptance agent"]
    })
}

fn response(value: Value) -> TranscriptionPipelineResponse {
    serde_json::from_value(value).expect("recorded JSON matches the upstream response shape")
}

fn failed(outcome: Result<SpeechRun, ProducerError>) -> String {
    match outcome {
        Err(ProducerError::Failed { reason }) => {
            assert!(!reason.trim().is_empty());
            reason
        }
        other => panic!("expected ProducerError::Failed, got {other:?}"),
    }
}

#[test]
fn recorded_response_maps_to_exact_segments_and_receipt() {
    let run = speech_run_from_response(&identity(), INPUT_HASH, &response(recorded())).unwrap();

    let receipt = &run.receipt;
    let id = identity();
    assert_eq!(receipt.modality, Modality::Speech);
    assert_eq!(receipt.producer, id.producer);
    assert_eq!(receipt.producer_version, id.producer_version);
    assert_eq!(receipt.model, id.model);
    assert_eq!(receipt.model_version, id.model_version);
    assert_eq!(receipt.config_hash, id.config_hash);
    assert_eq!(receipt.input_hash, INPUT_HASH);

    assert_eq!(
        run.segments,
        vec![
            TranscriptSegment {
                index: 0,
                start_seconds: 0.5,
                end_seconds: 2.25,
                text: "hello there".to_string(),
                language: Some("en".to_string()),
                speaker: None,
                confidence: Some(0.875),
            },
            TranscriptSegment {
                index: 1,
                start_seconds: 2.5,
                end_seconds: 4.0,
                text: "general kenobi".to_string(),
                language: Some("de".to_string()),
                speaker: Some("SPEAKER_00".to_string()),
                confidence: Some(0.5),
            },
            TranscriptSegment {
                index: 2,
                start_seconds: 5.0,
                end_seconds: 6.125,
                text: "no confidence".to_string(),
                language: Some("en".to_string()),
                speaker: None,
                confidence: None,
            },
        ]
    );
}

#[test]
fn mapping_agrees_with_the_core_timed_text_adoption() {
    // The adapter must go through the core's Foundation timed-text adoption, not a
    // parallel mapping.
    let recorded = response(recorded());
    let run = speech_run_from_response(&identity(), INPUT_HASH, &recorded).unwrap();
    let expected = SpeechRun::from_timed_text(run.receipt.clone(), &recorded.transcript).unwrap();
    assert_eq!(run, expected);
}

#[test]
fn empty_transcript_maps_to_an_empty_run() {
    let mut value = recorded();
    value["transcript"]["segments"] = json!([]);
    value["transcript"]["text"] = json!("");
    let run = speech_run_from_response(&identity(), INPUT_HASH, &response(value)).unwrap();
    assert!(run.segments.is_empty());
    assert_eq!(run.receipt.input_hash, INPUT_HASH);
}

#[test]
fn unaccepted_response_fails_with_its_diagnostics() {
    let mut value = recorded();
    value["accepted"] = json!(false);
    value["diagnostics"] = json!(["model load failed: tokenizer.json missing"]);
    let reason = failed(speech_run_from_response(
        &identity(),
        INPUT_HASH,
        &response(value),
    ));
    assert!(
        reason.contains("model load failed: tokenizer.json missing"),
        "{reason}"
    );
}

#[test]
fn response_from_another_provider_or_model_fails() {
    let mut other_provider = recorded();
    other_provider["provider"] = json!("whisper-cpp");
    failed(speech_run_from_response(
        &identity(),
        INPUT_HASH,
        &response(other_provider),
    ));

    let mut other_model = recorded();
    other_model["modelId"] = json!("openai/whisper-large-v3");
    failed(speech_run_from_response(
        &identity(),
        INPUT_HASH,
        &response(other_model),
    ));
}

#[test]
fn untimed_segment_fails_instead_of_being_placed_on_the_timeline() {
    let mut value = recorded();
    let segment = &mut value["transcript"]["segments"][1];
    segment.as_object_mut().unwrap().remove("startSeconds");
    segment.as_object_mut().unwrap().remove("endSeconds");
    failed(speech_run_from_response(
        &identity(),
        INPUT_HASH,
        &response(value),
    ));
}
