//! #32: no first-run downloads or network access unless the caller explicitly allows it.
//!
//! Download-capable configurations are refused at construction, before any run starts;
//! these tests never run a producer, so no network or model access can happen here.

#![cfg(feature = "audio-analysis")]

mod speech_common;

use std::path::PathBuf;

use media_intelligence_analysis::speech::transcription::{
    CandleWhisperOptions, WhisperXCommandOptions,
};
use media_intelligence_analysis::speech::{
    AudioAnalysisSpeechConfig, AudioAnalysisSpeechProducer, ModelFetchPolicy, SpeechAdapterError,
    TranscriptionProviderSelection,
};
use speech_common::*;

fn candle(options: CandleWhisperOptions) -> AudioAnalysisSpeechConfig {
    AudioAnalysisSpeechConfig::new(TranscriptionProviderSelection::CandleWhisper(options))
}

fn whisperx(options: WhisperXCommandOptions) -> AudioAnalysisSpeechConfig {
    AudioAnalysisSpeechConfig::new(TranscriptionProviderSelection::ExternalWhisperX(options))
}

fn refusal(config: AudioAnalysisSpeechConfig) -> String {
    match AudioAnalysisSpeechProducer::new(config) {
        Err(SpeechAdapterError::ModelFetchNotAllowed { reason }) => {
            assert!(!reason.trim().is_empty());
            reason
        }
        Err(other) => panic!("expected ModelFetchNotAllowed, got {other:?}"),
        Ok(_) => panic!("a download-capable configuration must be refused without permission"),
    }
}

fn allowed(mut config: AudioAnalysisSpeechConfig) {
    config.model_fetch = ModelFetchPolicy::AllowNetwork;
    AudioAnalysisSpeechProducer::new(config)
        .expect("the same configuration is accepted once the caller allows network fetching");
}

#[test]
fn candle_without_bundle_or_cache_only_is_refused_unless_allowed() {
    // audio-analysis' default Candle options resolve the model through the Hugging Face
    // downloader: no bundle, not cache-only.
    let config = candle(CandleWhisperOptions::default());
    let reason = refusal(config.clone());
    assert!(
        reason.contains("model_bundle") && reason.contains("model_cache_only"),
        "the reason must name the local alternatives: {reason}"
    );
    // A model directory alone is only a download cache; still download-capable.
    let reason = refusal(candle(CandleWhisperOptions {
        model_dir: Some(PathBuf::from("/models/hf-cache")),
        ..CandleWhisperOptions::default()
    }));
    assert!(reason.contains("model_cache_only"), "{reason}");
    allowed(config);
}

#[test]
fn local_candle_configurations_are_accepted_without_permission() {
    AudioAnalysisSpeechProducer::new(candle(CandleWhisperOptions {
        model_cache_only: true,
        ..CandleWhisperOptions::default()
    }))
    .expect("cache-only resolution never downloads");
    AudioAnalysisSpeechProducer::new(candle(CandleWhisperOptions {
        model_bundle: Some(PathBuf::from("/models/acceptance/whisper-tiny-bundle")),
        ..CandleWhisperOptions::default()
    }))
    .expect("an explicit bundle never downloads");
}

#[test]
fn whisperx_without_cache_only_is_refused_unless_allowed() {
    let config = whisperx(WhisperXCommandOptions::default());
    let reason = refusal(config.clone());
    assert!(reason.contains("model_cache_only"), "{reason}");
    allowed(config);
    AudioAnalysisSpeechProducer::new(whisperx(WhisperXCommandOptions {
        model_cache_only: true,
        no_align: true,
        diarize: false,
        ..WhisperXCommandOptions::default()
    }))
    .expect("cache-only WhisperX without alignment or diarization is local");
}

#[test]
fn cache_only_whisperx_with_alignment_or_diarization_is_refused_unless_allowed() {
    // Upstream WhisperX ignores `model_cache_only` for torchaudio alignment bundles
    // (`bundle.get_model` in whisperx/alignment.py `load_align_model`, the default for
    // English) and for the pyannote `DiarizationPipeline` (whisperx/transcribe.py), so
    // `model_cache_only` alone does not keep these stages off the network.
    let aligned = whisperx(WhisperXCommandOptions {
        model_cache_only: true,
        no_align: false,
        diarize: false,
        ..WhisperXCommandOptions::default()
    });
    refusal(aligned.clone());
    allowed(aligned);

    let diarized = whisperx(WhisperXCommandOptions {
        model_cache_only: true,
        no_align: true,
        diarize: true,
        ..WhisperXCommandOptions::default()
    });
    refusal(diarized.clone());
    allowed(diarized);
}

#[test]
fn whisper_cpp_never_downloads_and_is_accepted() {
    // An absent model path is a run-time failure (see speech_adapter_execution.rs), not a
    // download.
    AudioAnalysisSpeechProducer::new(AudioAnalysisSpeechConfig::new(whisper_cpp(None)))
        .expect("whisper.cpp only loads an explicit model path");
}

#[test]
fn download_capable_alignment_is_refused_unless_allowed() {
    let mut config = AudioAnalysisSpeechConfig::new(candle_bundle("/models/whisper-bundle"));
    config.alignment.enabled = true;
    let reason = refusal(config.clone());
    assert!(reason.contains("alignment"), "{reason}");
    allowed(config.clone());

    let mut cache_only = config.clone();
    cache_only.alignment.model_cache_only = true;
    AudioAnalysisSpeechProducer::new(cache_only).expect("cache-only alignment is local");
    let mut bundled = config;
    bundled.alignment.model_bundle = Some(PathBuf::from("/models/wav2vec2-bundle"));
    AudioAnalysisSpeechProducer::new(bundled).expect("bundled alignment is local");

    // Disabled alignment never resolves a model, whatever its options say.
    let disabled = AudioAnalysisSpeechConfig::new(candle_bundle("/models/whisper-bundle"));
    assert!(!disabled.alignment.enabled);
    assert!(disabled.alignment.model_bundle.is_none() && !disabled.alignment.model_cache_only);
    AudioAnalysisSpeechProducer::new(disabled).expect("disabled alignment is ignored");
}
