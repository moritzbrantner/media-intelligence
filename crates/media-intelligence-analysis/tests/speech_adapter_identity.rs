//! #32: producer identity and configuration hash of the audio-analysis speech adapter.
//! Pure configuration tests; nothing runs.

#![cfg(feature = "audio-analysis")]

mod speech_common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use media_intelligence_analysis::speech::transcription::{
    CandleWhisperComputeType, CandleWhisperDecodeRuntime, CandleWhisperOptions,
    NativeDevicePreference, TranscriptionTask, WhisperXCommandOptions,
};
use media_intelligence_analysis::speech::{
    AudioAnalysisSpeechConfig, AudioAnalysisSpeechProducer, ModelFetchPolicy,
    TranscriptionProviderSelection, TranscriptionSource, AUDIO_ANALYSIS_REVISION,
    SPEECH_PRODUCER_PREFIX, UNSPECIFIED_MODEL_VERSION,
};
use media_intelligence_analysis::{ProducerIdentity, SpeechProducer};
use speech_common::*;

const BUNDLE: &str = "/models/acceptance/whisper-tiny-bundle";

fn base_options() -> CandleWhisperOptions {
    match candle_bundle(BUNDLE) {
        TranscriptionProviderSelection::CandleWhisper(options) => options,
        _ => unreachable!(),
    }
}

fn base_config() -> AudioAnalysisSpeechConfig {
    AudioAnalysisSpeechConfig::new(candle_bundle(BUNDLE))
}

fn with_candle(change: impl FnOnce(&mut CandleWhisperOptions)) -> AudioAnalysisSpeechConfig {
    let mut options = base_options();
    change(&mut options);
    AudioAnalysisSpeechConfig::new(TranscriptionProviderSelection::CandleWhisper(options))
}

#[test]
fn constants_pin_the_exact_audio_analysis_revision() {
    assert_pinned_revision();
    assert_eq!(SPEECH_PRODUCER_PREFIX, "audio-analysis");
    assert_eq!(UNSPECIFIED_MODEL_VERSION, "unspecified");
}

#[test]
fn new_config_uses_defaults_and_local_only_policy() {
    let config = base_config();
    assert_eq!(config.provider, candle_bundle(BUNDLE));
    assert_eq!(config.vad, Default::default());
    assert_eq!(config.alignment, Default::default());
    assert!(!config.alignment.enabled);
    assert_eq!(config.diarization, Default::default());
    assert!(!config.diarization.enabled);
    assert_eq!(config.model_version, None);
    assert_eq!(config.model_fetch, ModelFetchPolicy::LocalOnly);
    assert_eq!(ModelFetchPolicy::default(), ModelFetchPolicy::LocalOnly);
}

#[test]
fn identity_names_provider_revision_model_and_config_hash() {
    let config = base_config();
    let hash = config.config_hash();
    assert!(is_sha256_prefixed(&hash), "{hash}");
    let producer = producer_with(config.clone());
    assert_eq!(producer.config(), &config);
    assert_eq!(
        producer.identity(),
        ProducerIdentity {
            producer: "audio-analysis:candle-whisper".to_string(),
            producer_version: EXPECTED_AUDIO_ANALYSIS_REVISION.to_string(),
            model: "openai/whisper-tiny".to_string(),
            model_version: "unspecified".to_string(),
            config_hash: hash.clone(),
        }
    );

    // A declared model version is reported verbatim and is not part of the config hash.
    let mut declared = config.clone();
    declared.model_version = Some("whisper-tiny@169d4a4".to_string());
    let identity = producer_with(declared.clone()).identity();
    assert_eq!(identity.model_version, "whisper-tiny@169d4a4");
    assert_eq!(identity.config_hash, hash);
    assert_eq!(declared.config_hash(), hash);
}

#[test]
fn identity_follows_the_selected_provider() {
    let cpp = producer(whisper_cpp(Some(PathBuf::from("/models/ggml-tiny.bin")))).identity();
    assert_eq!(cpp.producer, "audio-analysis:whisper-cpp");
    assert_eq!(cpp.producer_version, AUDIO_ANALYSIS_REVISION);
    assert_eq!(cpp.model, "ggml-tiny");

    let whisperx = producer(TranscriptionProviderSelection::ExternalWhisperX(
        WhisperXCommandOptions {
            model: "large-v2".to_string(),
            model_dir: Some(PathBuf::from("/models/whisperx")),
            model_cache_only: true,
            no_align: true,
            ..WhisperXCommandOptions::default()
        },
    ))
    .identity();
    assert_eq!(whisperx.producer, "audio-analysis:whisperx-command");
    assert_eq!(whisperx.model, "large-v2");
    assert_ne!(cpp.config_hash, whisperx.config_hash);
}

#[test]
fn config_hash_is_deterministic_for_equal_configurations() {
    let first = base_config();
    let second = with_candle(|_| {});
    assert_eq!(first, second);
    assert_eq!(first.config_hash(), second.config_hash());
    assert_eq!(first.config_hash(), first.config_hash());
    assert_eq!(
        producer_with(first).identity(),
        producer_with(second).identity()
    );
}

#[test]
fn every_effective_option_changes_the_config_hash() {
    let base = base_config();
    let base_hash = base.config_hash();
    let mut variants: BTreeMap<&str, AudioAnalysisSpeechConfig> = BTreeMap::new();
    variants.insert(
        "model id",
        with_candle(|o| o.model_id = "openai/whisper-base".into()),
    );
    variants.insert("language", with_candle(|o| o.language = Some("de".into())));
    variants.insert(
        "task",
        with_candle(|o| o.task = TranscriptionTask::Translate),
    );
    variants.insert(
        "compute type",
        with_candle(|o| o.compute_type = CandleWhisperComputeType::Fp32),
    );
    variants.insert(
        "device",
        with_candle(|o| o.device = NativeDevicePreference::Cpu),
    );
    variants.insert(
        "model bundle",
        with_candle(|o| o.model_bundle = Some("/models/acceptance/other-bundle".into())),
    );
    variants.insert(
        "batch chunks",
        with_candle(|o| o.batch_chunks = !o.batch_chunks),
    );
    variants.insert(
        "max batch size",
        with_candle(|o| o.max_batch_size = Some(8)),
    );
    variants.insert(
        "decode runtime",
        with_candle(|o| o.decode_runtime = CandleWhisperDecodeRuntime::ActiveRowTensorBatch),
    );
    let mut vad_enabled = base.clone();
    vad_enabled.vad.enabled = !vad_enabled.vad.enabled;
    variants.insert("vad enabled", vad_enabled);
    let mut vad_threshold = base.clone();
    vad_threshold.vad.rms_threshold *= 2.0;
    variants.insert("vad threshold", vad_threshold);
    let mut vad_chunk = base.clone();
    vad_chunk.vad.max_chunk_seconds += 1.0;
    variants.insert("vad max chunk", vad_chunk);
    let mut alignment = base.clone();
    alignment.alignment.enabled = true;
    alignment.alignment.model_bundle = Some("/models/acceptance/wav2vec2-bundle".into());
    variants.insert("alignment", alignment);
    let mut diarization = base.clone();
    diarization.diarization.enabled = true;
    variants.insert("diarization", diarization);
    variants.insert(
        "provider",
        AudioAnalysisSpeechConfig::new(whisper_cpp(Some("/models/ggml-tiny.bin".into()))),
    );

    let mut seen = BTreeMap::new();
    for (option, config) in &variants {
        let hash = config.config_hash();
        assert!(is_sha256_prefixed(&hash), "{option}: {hash}");
        assert_ne!(
            hash, base_hash,
            "changing the {option} must change the hash"
        );
        if let Some(previous) = seen.insert(hash, *option) {
            panic!("{option} and {previous} hash identically");
        }
    }
}

#[test]
fn permission_and_declared_version_do_not_change_the_config_hash() {
    let base = base_config();
    let mut allowed = base.clone();
    allowed.model_fetch = ModelFetchPolicy::AllowNetwork;
    assert_eq!(allowed.config_hash(), base.config_hash());
    assert_eq!(
        producer_with(allowed).identity(),
        producer_with(base.clone()).identity(),
        "permission to download is not part of the producer identity"
    );

    let mut versioned = base.clone();
    versioned.model_version = Some("v1".into());
    assert_eq!(versioned.config_hash(), base.config_hash());
    assert_ne!(
        producer_with(versioned).identity(),
        producer_with(base).identity(),
        "the declared model version is part of the identity"
    );
}

#[test]
fn transcription_request_carries_the_configured_options() {
    let mut config = with_candle(|o| o.language = Some("en".into()));
    config.vad.enabled = false;
    config.alignment.enabled = true;
    config.alignment.model_bundle = Some("/models/acceptance/wav2vec2-bundle".into());
    let producer = AudioAnalysisSpeechProducer::new(config.clone()).unwrap();
    let media = Path::new("/media/acceptance/clip.mp4");
    let request = producer.transcription_request(media);
    assert_eq!(
        request.source,
        TranscriptionSource::Path {
            path: media.to_path_buf()
        }
    );
    assert_eq!(request.provider, config.provider);
    assert_eq!(request.vad, config.vad);
    assert_eq!(request.alignment, config.alignment);
    assert_eq!(request.diarization, config.diarization);
    // The media path selects the input, not the configuration.
    let other = producer.transcription_request(Path::new("/media/acceptance/other.mp4"));
    assert_eq!(other.provider, request.provider);
    assert_eq!(producer.identity().config_hash, config.config_hash());
}
