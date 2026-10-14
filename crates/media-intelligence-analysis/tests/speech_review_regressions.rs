//! Implementation-authored regressions for the Codex review on PR #37 (not part of the
//! independent acceptance contract).
#![cfg(feature = "audio-analysis")]

use media_intelligence_analysis::speech::transcription::{
    CandleWhisperOptions, TranscriptionProviderSelection, WhisperXCommandOptions,
};
use media_intelligence_analysis::speech::{
    AudioAnalysisSpeechConfig, AudioAnalysisSpeechProducer, SpeechAdapterError,
};
use media_intelligence_analysis::{
    analyze, AnalysisProfile, AnalysisRequest, AssetId, CancellationToken, ContentHash,
    InMemoryStageCache, MediaIdentity, Producers, StageStatus,
};

#[test]
fn whisperx_diarization_is_refused_without_network_permission() {
    let provider = TranscriptionProviderSelection::ExternalWhisperX(WhisperXCommandOptions {
        model_cache_only: true,
        diarize: true,
        ..WhisperXCommandOptions::default()
    });
    assert!(matches!(
        AudioAnalysisSpeechProducer::new(AudioAnalysisSpeechConfig::new(provider)),
        Err(SpeechAdapterError::ModelFetchNotAllowed { .. })
    ));
}

#[test]
fn disabled_stage_settings_do_not_change_the_config_hash() {
    let provider = TranscriptionProviderSelection::CandleWhisper(CandleWhisperOptions {
        model_bundle: Some("/models/whisper".into()),
        ..CandleWhisperOptions::default()
    });
    let base = AudioAnalysisSpeechConfig::new(provider);
    assert!(!base.alignment.enabled && !base.diarization.enabled);

    let mut changed = base.clone();
    changed.alignment.model_bundle = Some("/elsewhere/align".into());
    changed.alignment.model_id = "other-align-model".to_string();
    assert_eq!(changed.config_hash(), base.config_hash());

    let mut vad_off = base.clone();
    vad_off.vad.enabled = false;
    let mut vad_off_other = vad_off.clone();
    vad_off_other.vad.rms_threshold += 0.25;
    assert_eq!(vad_off.config_hash(), vad_off_other.config_hash());
    assert_ne!(vad_off.config_hash(), base.config_hash());
}

#[test]
fn stale_model_dir_is_ignored_when_a_bundle_is_selected() {
    let dir = tempfile::tempdir().unwrap();
    let bundle = dir.path().join("bundle");
    std::fs::create_dir(&bundle).unwrap();
    let media = dir.path().join("media.bin");
    std::fs::write(&media, b"not audio").unwrap();
    let provider = TranscriptionProviderSelection::CandleWhisper(CandleWhisperOptions {
        model_bundle: Some(bundle),
        model_dir: Some(dir.path().join("does-not-exist")),
        ..CandleWhisperOptions::default()
    });
    let producer =
        AudioAnalysisSpeechProducer::new(AudioAnalysisSpeechConfig::new(provider)).unwrap();
    let report = analyze(
        &AnalysisRequest {
            media: MediaIdentity {
                asset_id: AssetId::new("asset").unwrap(),
                content_hash: ContentHash::from_bytes(b"not audio"),
                duration_seconds: None,
            },
            media_path: media,
            profile: AnalysisProfile {
                speech: true,
                visible_text: false,
                scenes: None,
            },
        },
        &Producers {
            speech: Some(&producer),
            visible_text: None,
        },
        &mut InMemoryStageCache::new(),
        &CancellationToken::new(),
    )
    .unwrap();
    // The run still fails (the bundle is empty and the media has no audio), but the
    // failure must come from audio-analysis, not from the unused `model_dir`.
    match report.speech {
        StageStatus::Failed { reason, .. } => {
            assert!(!reason.contains("model_dir"), "{reason}")
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn whisperx_extra_args_are_refused_without_network_permission() {
    let provider = TranscriptionProviderSelection::ExternalWhisperX(WhisperXCommandOptions {
        model_cache_only: true,
        no_align: true,
        diarize: false,
        extra_args: vec!["--vad_method".to_string(), "silero".to_string()],
        ..WhisperXCommandOptions::default()
    });
    assert!(matches!(
        AudioAnalysisSpeechProducer::new(AudioAnalysisSpeechConfig::new(provider)),
        Err(SpeechAdapterError::ModelFetchNotAllowed { .. })
    ));
}
