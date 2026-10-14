//! #32: the audio-analysis speech adapter inside `analyze`, on a real (ffmpeg-generated,
//! video-only) clip with real scene detection.
//!
//! No ASR model exists on the test machine and none is executed: every speech run here
//! either fails before audio-analysis can load a model, is cancelled, or is served from a
//! cache entry planted by the test.

#![cfg(feature = "audio-analysis")]

mod common;
mod speech_common;

use common::*;
use media_intelligence_analysis::speech::transcription::CandleWhisperOptions;
use media_intelligence_analysis::speech::{
    AudioAnalysisSpeechConfig, AudioAnalysisSpeechProducer, TranscriptionProviderSelection,
};
use media_intelligence_analysis::{
    analyze, input_hash, AnalysisProfile, AnalysisReport, CachedStage, CancellationToken,
    ContentHash, InMemoryStageCache, MediaIdentity, Modality, ProducerError, ProducerInput,
    ProducerReceipt, Producers, SceneConfig, SpeechProducer, SpeechRun, StageCache, StageKey,
    StageStatus,
};
use media_intelligence_core::{CoverageStatus, TranscriptSegment};
use speech_common::*;

fn speech_and_scenes() -> AnalysisProfile {
    AnalysisProfile {
        speech: true,
        visible_text: false,
        scenes: Some(SceneConfig::default()),
    }
}

fn run(
    clip: &Clip,
    producer: &AudioAnalysisSpeechProducer,
    cache: &mut InMemoryStageCache,
    token: &CancellationToken,
) -> AnalysisReport {
    analyze(
        &request(clip, speech_and_scenes()),
        &Producers {
            speech: Some(producer),
            visible_text: None,
        },
        cache,
        token,
    )
    .expect("integrity checks pass; stage outcomes are reported per stage")
}

fn failure_reason(status: &StageStatus) -> &str {
    match status {
        StageStatus::Failed { reason, .. } => {
            assert!(!reason.trim().is_empty(), "the failure must carry a reason");
            reason
        }
        other => panic!("expected speech Failed, got {other:?}"),
    }
}

fn assert_scenes_completed(report: &AnalysisReport) {
    match &report.scenes {
        StageStatus::CompletedWithFindings { evidence_count, .. } => {
            assert_eq!(*evidence_count, 3)
        }
        other => panic!("scenes must complete independently of speech, got {other:?}"),
    }
    assert_eq!(count(&report.analysis, Modality::Scene), 3);
}

fn assert_nothing_cached_for_speech(cache: &InMemoryStageCache) {
    assert!(
        cache
            .keys()
            .iter()
            .all(|key| key.modality != Modality::Speech),
        "a failed or cancelled speech stage must not be cached"
    );
}

#[test]
fn missing_candle_bundle_fails_speech_with_actionable_reason_while_scenes_complete() {
    let Some(clip) = three_scene_clip("speech_missing_bundle") else {
        return;
    };
    let models = tempfile::tempdir().unwrap();
    let bundle = missing(models.path(), "whisper-tiny-bundle");
    let producer = producer(candle_bundle(&bundle));
    let mut cache = InMemoryStageCache::new();
    let report = run(&clip, &producer, &mut cache, &CancellationToken::new());

    let reason = failure_reason(&report.speech);
    assert!(
        reason.contains("model_bundle") && reason.contains(&bundle.display().to_string()),
        "the reason must name the option and the missing path: {reason}"
    );
    assert_eq!(count(&report.analysis, Modality::Speech), 0);
    assert!(matches!(
        coverage(&report.analysis, Modality::Speech),
        CoverageStatus::Failed { .. }
    ));
    assert_scenes_completed(&report);
    assert_eq!(cache.len(), 1, "only the scene run is cached");
    assert_nothing_cached_for_speech(&cache);

    // Failures are not cached: an unchanged rerun tries speech again; scenes are reused.
    let rerun = run(&clip, &producer, &mut cache, &CancellationToken::new());
    failure_reason(&rerun.speech);
    assert!(matches!(rerun.scenes, StageStatus::Reused { .. }));
    assert_nothing_cached_for_speech(&cache);
}

#[test]
fn missing_whisper_cpp_model_path_fails_speech_with_actionable_reason() {
    let Some(clip) = three_scene_clip("speech_missing_ggml") else {
        return;
    };
    let models = tempfile::tempdir().unwrap();

    let absent = producer(whisper_cpp(None));
    let mut cache = InMemoryStageCache::new();
    let report = run(&clip, &absent, &mut cache, &CancellationToken::new());
    let reason = failure_reason(&report.speech);
    assert!(reason.contains("model_path"), "{reason}");
    assert_scenes_completed(&report);
    assert_nothing_cached_for_speech(&cache);

    let model = missing(models.path(), "ggml-tiny.bin");
    let missing_file = producer(whisper_cpp(Some(model.clone())));
    let report = run(&clip, &missing_file, &mut cache, &CancellationToken::new());
    let reason = failure_reason(&report.speech);
    assert!(
        reason.contains("model_path") && reason.contains(&model.display().to_string()),
        "{reason}"
    );
    assert!(matches!(report.scenes, StageStatus::Reused { .. }));
    assert_nothing_cached_for_speech(&cache);
}

#[test]
fn missing_cache_only_model_dir_fails_speech_naming_the_directory() {
    let Some(clip) = three_scene_clip("speech_missing_model_dir") else {
        return;
    };
    let models = tempfile::tempdir().unwrap();
    let model_dir = missing(models.path(), "hf-cache");
    let producer = producer(TranscriptionProviderSelection::CandleWhisper(
        CandleWhisperOptions {
            model_dir: Some(model_dir.clone()),
            model_cache_only: true,
            ..CandleWhisperOptions::default()
        },
    ));
    let mut cache = InMemoryStageCache::new();
    let report = run(&clip, &producer, &mut cache, &CancellationToken::new());
    let reason = failure_reason(&report.speech);
    assert!(
        reason.contains("model_dir") && reason.contains(&model_dir.display().to_string()),
        "{reason}"
    );
    assert_scenes_completed(&report);
    assert_nothing_cached_for_speech(&cache);
}

#[test]
fn audio_analysis_pipeline_error_fails_speech_only() {
    let Some(clip) = three_scene_clip("speech_pipeline_error") else {
        return;
    };
    // The bundle exists (empty), so the adapter's own checks pass and audio-analysis is
    // called. The clip has no audio stream and the bundle no model files, so the
    // pipeline must fail; its error becomes the speech failure.
    let models = tempfile::tempdir().unwrap();
    let producer = producer(candle_bundle(models.path()));
    let mut cache = InMemoryStageCache::new();
    let report = run(&clip, &producer, &mut cache, &CancellationToken::new());
    failure_reason(&report.speech);
    assert_eq!(count(&report.analysis, Modality::Speech), 0);
    assert_scenes_completed(&report);
    assert_nothing_cached_for_speech(&cache);
}

#[test]
fn cancelled_before_the_run_returns_cancelled_without_calling_audio_analysis() {
    // Direct call: the media path and the bundle do not exist, so any file-system or
    // audio-analysis work would yield `Failed`. `Cancelled` proves the token is checked
    // first.
    let scratch = tempfile::tempdir().unwrap();
    let producer = producer(candle_bundle(missing(scratch.path(), "bundle")));
    let media_path = missing(scratch.path(), "clip.mp4");
    let media = MediaIdentity {
        asset_id: media_intelligence_analysis::AssetId::new("acceptance:issue-32").unwrap(),
        content_hash: ContentHash::from_bytes(b"not read"),
        duration_seconds: None,
    };
    let hash = input_hash(&media.content_hash);
    let token = CancellationToken::new();
    token.cancel();
    let outcome = producer.run(&ProducerInput {
        media: &media,
        media_path: &media_path,
        input_hash: &hash,
        cancellation: &token,
        scenes: None,
    });
    assert_eq!(outcome, Err(ProducerError::Cancelled));
}

#[test]
fn cancelled_analysis_reports_speech_cancelled_and_caches_nothing() {
    let Some(clip) = three_scene_clip("speech_cancelled") else {
        return;
    };
    let models = tempfile::tempdir().unwrap();
    let producer = producer(candle_bundle(missing(models.path(), "bundle")));
    let token = CancellationToken::new();
    token.cancel();
    let mut cache = InMemoryStageCache::new();
    let report = run(&clip, &producer, &mut cache, &token);
    assert_eq!(report.speech, StageStatus::Cancelled);
    assert_eq!(report.scenes, StageStatus::Cancelled);
    assert!(report.analysis.evidence.is_empty());
    assert!(cache.is_empty());
}

fn planted_speech(producer: &AudioAnalysisSpeechProducer, clip: &Clip) -> SpeechRun {
    let identity = producer.identity();
    let segment = |index, start, end, text: &str| TranscriptSegment {
        index,
        start_seconds: start,
        end_seconds: end,
        text: text.to_string(),
        language: Some("en".to_string()),
        speaker: None,
        confidence: Some(0.75),
    };
    SpeechRun {
        receipt: ProducerReceipt {
            modality: Modality::Speech,
            producer: identity.producer,
            producer_version: identity.producer_version,
            model: identity.model,
            model_version: identity.model_version,
            config_hash: identity.config_hash,
            input_hash: input_hash(&clip.content_hash),
            run_id: Some("planted-by-acceptance-test".to_string()),
            config: None,
        },
        segments: vec![
            segment(0, 1.0, 4.0, "planted red"),
            segment(1, 6.0, 9.0, "planted green"),
        ],
    }
}

#[test]
fn unchanged_rerun_reuses_cached_speech_and_changed_config_re_executes_speech_only() {
    let Some(clip) = three_scene_clip("speech_reuse") else {
        return;
    };
    // The bundle is missing, so executing this producer could only fail. A `Reused`
    // status with the planted run therefore proves the producer was not executed.
    let models = tempfile::tempdir().unwrap();
    let bundle = missing(models.path(), "bundle");
    let producer = producer(candle_bundle(&bundle));
    let planted = planted_speech(&producer, &clip);
    let mut cache = InMemoryStageCache::new();
    cache.put(
        StageKey {
            modality: Modality::Speech,
            content_hash: clip.content_hash.clone(),
            producer: producer.identity(),
        },
        CachedStage::Speech(planted.clone()),
    );

    let first = run(&clip, &producer, &mut cache, &CancellationToken::new());
    match &first.speech {
        StageStatus::Reused {
            receipt,
            evidence_count,
        } => {
            assert_eq!(receipt, &planted.receipt);
            assert_eq!(*evidence_count, 2);
        }
        other => panic!("expected reused speech, got {other:?}"),
    }
    assert_eq!(count(&first.analysis, Modality::Speech), 2);
    assert_scenes_completed(&first);

    // Unchanged rerun: speech and scenes both reused, identical analysis.
    let second = run(&clip, &producer, &mut cache, &CancellationToken::new());
    assert!(matches!(second.speech, StageStatus::Reused { .. }));
    assert!(matches!(second.scenes, StageStatus::Reused { .. }));
    assert_eq!(first.analysis, second.analysis);

    // A changed decode option (language) is a new identity: speech re-executes (and, with
    // the bundle missing, fails), while scenes are still reused.
    let mut changed = AudioAnalysisSpeechConfig::new(candle_bundle(&bundle));
    if let TranscriptionProviderSelection::CandleWhisper(options) = &mut changed.provider {
        options.language = Some("de".to_string());
    }
    let changed = AudioAnalysisSpeechProducer::new(changed).unwrap();
    assert_ne!(
        changed.identity().config_hash,
        producer.identity().config_hash
    );
    let third = run(&clip, &changed, &mut cache, &CancellationToken::new());
    failure_reason(&third.speech);
    assert!(matches!(third.scenes, StageStatus::Reused { .. }));
    assert_eq!(
        evidence_ids(&first.analysis, Modality::Scene),
        evidence_ids(&third.analysis, Modality::Scene)
    );
    // The planted run is still the only speech entry.
    let speech_keys: Vec<StageKey> = cache
        .keys()
        .into_iter()
        .filter(|key| key.modality == Modality::Speech)
        .collect();
    assert_eq!(speech_keys.len(), 1);
    assert_eq!(speech_keys[0].producer, producer.identity());
}
