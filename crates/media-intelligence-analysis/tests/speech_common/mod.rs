//! Shared helpers for the #32 `audio-analysis` speech adapter acceptance tests.
//!
//! No model runs in these tests. Model locations are fictitious paths or empty temp
//! directories; transcripts are recorded JSON written by the acceptance agent.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use media_intelligence_analysis::speech::transcription::{
    CandleWhisperOptions, TranscriptionProviderSelection, WhisperCppProviderOptions,
};
use media_intelligence_analysis::speech::{
    AudioAnalysisSpeechConfig, AudioAnalysisSpeechProducer, AUDIO_ANALYSIS_REVISION,
};

pub const EXPECTED_AUDIO_ANALYSIS_REVISION: &str = "cc8075f633a4bda5d1b9902b3f4c4d07eca017d9";

/// Candle Whisper, `openai/whisper-tiny`, loaded from the given local bundle only.
pub fn candle_bundle(bundle: impl Into<PathBuf>) -> TranscriptionProviderSelection {
    TranscriptionProviderSelection::CandleWhisper(CandleWhisperOptions {
        model_id: "openai/whisper-tiny".to_string(),
        model_bundle: Some(bundle.into()),
        model_cache_only: true,
        ..CandleWhisperOptions::default()
    })
}

pub fn whisper_cpp(model_path: Option<PathBuf>) -> TranscriptionProviderSelection {
    TranscriptionProviderSelection::WhisperCpp(WhisperCppProviderOptions {
        model_id: "ggml-tiny".to_string(),
        model_path,
        ..WhisperCppProviderOptions::default()
    })
}

/// A local-only adapter; construction must succeed for these configurations.
pub fn producer(provider: TranscriptionProviderSelection) -> AudioAnalysisSpeechProducer {
    AudioAnalysisSpeechProducer::new(AudioAnalysisSpeechConfig::new(provider))
        .expect("local-only configuration is accepted")
}

pub fn producer_with(config: AudioAnalysisSpeechConfig) -> AudioAnalysisSpeechProducer {
    AudioAnalysisSpeechProducer::new(config).expect("local-only configuration is accepted")
}

/// A path inside `dir` that does not exist.
pub fn missing(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    assert!(!path.exists());
    path
}

pub fn assert_pinned_revision() {
    assert_eq!(AUDIO_ANALYSIS_REVISION, EXPECTED_AUDIO_ANALYSIS_REVISION);
}

pub fn is_sha256_prefixed(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}
