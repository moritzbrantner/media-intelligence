//! #34: **model-backed** acceptance on the committed CC BY 4.0 excerpt
//! (`tests/fixtures/model_acceptance/`, see its README for attribution).
//!
//! Real models run here: Candle Whisper through the `audio-analysis` speech adapter (#32)
//! and ONNX TrOCR through the `visual-analysis` visible-text adapter (#33), together with
//! the canonical scene producer, all inside one `analyze` call. Nothing is replayed or
//! faked, and nothing is downloaded: both adapters are built with
//! [`ModelFetchPolicy::LocalOnly`] from local model locations the caller supplies.
//!
//! # Opt-in contract
//!
//! Compiled only with the features `audio-analysis-candle`, `audio-analysis-media` (container
//! audio decoding through audio-analysis `audio-io`) and `visual-analysis-ocr-onnx`. At run
//! time every test prints `SKIPPED (model acceptance not configured)` and returns unless
//! `MI_MODEL_ACCEPTANCE=1`. When it is set, these are required (a missing or non-existent
//! location panics instead of skipping, so a configured run cannot silently prove nothing):
//!
//! * `MI_WHISPER_MODEL_DIR` – a local Candle Whisper bundle directory (`config.json`,
//!   `generation_config.json`, `tokenizer.json`, `preprocessor_config.json`,
//!   `model.safetensors`), passed as `CandleWhisperOptions::model_bundle`.
//! * `MI_TROCR_BUNDLE_ROOT` – the model-runtime bundle root holding
//!   `Xenova/trocr-base-printed` (`OcrLocalModelOptions::bundle_root`, `auto_download`
//!   off).
//!
//! Optional: `MI_WHISPER_MODEL_ID` (default `openai/whisper-base.en`, the receipt `model`)
//! and `MI_TROCR_CACHE_DIR` (`OcrLocalModelOptions::cache_dir`).
//!
//! ```sh
//! MI_MODEL_ACCEPTANCE=1 \
//! MI_WHISPER_MODEL_DIR=~/.cache/media-intelligence-acceptance/models/whisper-base.en \
//! MI_TROCR_BUNDLE_ROOT=~/.cache/media-intelligence-acceptance/models/trocr \
//! cargo test --release --locked -p media-intelligence-analysis \
//!   --features audio-analysis-candle,audio-analysis-media,visual-analysis-ocr-onnx \
//!   --test model_backed_acceptance -- --nocapture --test-threads=1
//! ```
//!
//! The model-running tests also serialise on a process-wide lock, so two models are never
//! loaded twice concurrently. The first test prints the full report JSON for evidence.

#![cfg(all(
    feature = "audio-analysis-candle",
    feature = "audio-analysis-media",
    feature = "visual-analysis-ocr-onnx"
))]

mod model_acceptance_common;

use std::path::PathBuf;
use std::sync::Mutex;

use media_intelligence_analysis::speech::{
    AudioAnalysisSpeechConfig, AudioAnalysisSpeechProducer, CandleWhisperOptions,
    TranscriptionProviderSelection, AUDIO_ANALYSIS_REVISION,
};
use media_intelligence_analysis::visible_text::{
    OcrBackendConfig, OcrLocalModelOptions, OcrPreset, OcrRequest, VisualAnalysisOcrConfig,
    VisualAnalysisOcrProducer, VISUAL_ANALYSIS_REVISION,
};
use media_intelligence_analysis::{
    analyze, input_hash, scene_bound_identity, AnalysisReport, CancellationToken, EvidenceAnalysis,
    InMemoryStageCache, Modality, ModelFetchPolicy, ProducerReceipt, Producers, SceneRun,
    SpeechProducer, StageStatus, VisibleTextProducer, SCENE_MODEL, SCENE_PRODUCER,
};
use media_intelligence_core::{
    CoverageStatus, EvidenceContent, EvidenceItem, RelationKind, SceneInterval,
};
use model_acceptance_common::*;

const SKIP_MESSAGE: &str = "SKIPPED (model acceptance not configured)";
const DEFAULT_WHISPER_MODEL_ID: &str = "openai/whisper-base.en";
const SPEECH_PRODUCER: &str = "audio-analysis:candle-whisper";
const OCR_PRODUCER: &str = "visual-analysis:onnx-trocr";

/// Real narration runs at roughly 2–3 words per second; 30 words in 32 s of near-continuous
/// speech (README: RMS above -25 dBFS in 31 of 32 seconds) is a deliberately low floor that
/// an empty, stubbed or hallucination-truncated transcript does not reach.
const MIN_TRANSCRIPT_WORDS: usize = 30;
/// Segments must cover at least half of the excerpt: the audio is speech nearly throughout.
const MIN_SPEECH_COVERAGE_SECONDS: f64 = FIXTURE_DURATION_SECONDS / 2.0;

static MODEL_LOCK: Mutex<()> = Mutex::new(());

/// Local model locations from the environment, or `None` (after printing the skip message)
/// when model acceptance is not enabled.
struct ModelEnv {
    whisper_bundle: PathBuf,
    whisper_model_id: String,
    trocr_bundle_root: PathBuf,
    trocr_cache_dir: Option<PathBuf>,
}

fn model_env(test: &str) -> Option<ModelEnv> {
    if std::env::var("MI_MODEL_ACCEPTANCE").ok().as_deref() != Some("1") {
        eprintln!("{SKIP_MESSAGE}: {test} (set MI_MODEL_ACCEPTANCE=1 to run)");
        return None;
    }
    let required = |name: &str| -> PathBuf {
        let value = std::env::var_os(name).unwrap_or_else(|| {
            panic!("MI_MODEL_ACCEPTANCE=1 but {name} is not set; see the module docs")
        });
        let path = PathBuf::from(value);
        assert!(
            path.exists(),
            "{name} `{}` does not exist; model acceptance never downloads models",
            path.display()
        );
        path
    };
    assert!(
        ffmpeg_available(),
        "MI_MODEL_ACCEPTANCE=1 but ffmpeg/ffprobe are not on PATH"
    );
    Some(ModelEnv {
        whisper_bundle: required("MI_WHISPER_MODEL_DIR"),
        whisper_model_id: std::env::var("MI_WHISPER_MODEL_ID")
            .unwrap_or_else(|_| DEFAULT_WHISPER_MODEL_ID.to_string()),
        trocr_bundle_root: required("MI_TROCR_BUNDLE_ROOT"),
        trocr_cache_dir: std::env::var_os("MI_TROCR_CACHE_DIR").map(PathBuf::from),
    })
}

fn speech_with_bundle(env: &ModelEnv, bundle: PathBuf) -> AudioAnalysisSpeechProducer {
    let options = CandleWhisperOptions {
        model_id: env.whisper_model_id.clone(),
        model_bundle: Some(bundle),
        ..CandleWhisperOptions::default()
    };
    let config =
        AudioAnalysisSpeechConfig::new(TranscriptionProviderSelection::CandleWhisper(options));
    assert_eq!(config.model_fetch, ModelFetchPolicy::LocalOnly);
    AudioAnalysisSpeechProducer::new(config).expect("a local Whisper bundle is never refused")
}

fn speech(env: &ModelEnv) -> AudioAnalysisSpeechProducer {
    speech_with_bundle(env, env.whisper_bundle.clone())
}

fn ocr_config(
    env: &ModelEnv,
    bundle_root: PathBuf,
    request: OcrRequest,
) -> VisualAnalysisOcrConfig {
    let options = OcrLocalModelOptions {
        preset: OcrPreset::TrOcrBasePrintedOnnx,
        bundle_root,
        auto_download: false,
        cache_dir: env.trocr_cache_dir.clone(),
        ..OcrLocalModelOptions::default()
    };
    let mut config = VisualAnalysisOcrConfig::new(OcrBackendConfig::OnnxTrOcr(options));
    config.request = request;
    assert_eq!(config.model_fetch, ModelFetchPolicy::LocalOnly);
    config
}

fn ocr_with(
    env: &ModelEnv,
    bundle_root: PathBuf,
    request: OcrRequest,
) -> VisualAnalysisOcrProducer {
    VisualAnalysisOcrProducer::new(ocr_config(env, bundle_root, request))
        .expect("TrOCR without auto_download is never refused")
}

fn ocr(env: &ModelEnv) -> VisualAnalysisOcrProducer {
    ocr_with(env, env.trocr_bundle_root.clone(), OcrRequest::default())
}

/// The same OCR configuration with only the request changed: one extra adapter attribute.
/// It changes the OCR config hash (the request is hashed) and nothing else.
fn changed_request() -> OcrRequest {
    let mut request = OcrRequest::default();
    request
        .attributes
        .insert("acceptance-variant".to_string(), "issue-34".to_string());
    request
}

fn run(
    speech: &AudioAnalysisSpeechProducer,
    ocr: &VisualAnalysisOcrProducer,
    profile: media_intelligence_analysis::AnalysisProfile,
    cache: &mut InMemoryStageCache,
    cancellation: &CancellationToken,
) -> AnalysisReport {
    analyze(
        &fixture_request(profile),
        &Producers {
            speech: Some(speech),
            visible_text: Some(ocr),
        },
        cache,
        cancellation,
    )
    .expect("analysis of the committed excerpt")
}

fn completed(status: &StageStatus, stage: &str) -> (ProducerReceipt, usize) {
    match status {
        StageStatus::CompletedWithFindings {
            receipt,
            evidence_count,
        } => (receipt.clone(), *evidence_count),
        other => panic!("{stage}: expected CompletedWithFindings, got {other:?}"),
    }
}

fn reused(status: &StageStatus, stage: &str) -> (ProducerReceipt, usize) {
    match status {
        StageStatus::Reused {
            receipt,
            evidence_count,
        } => (receipt.clone(), *evidence_count),
        other => panic!("{stage}: expected Reused, got {other:?}"),
    }
}

fn failed_reason(status: &StageStatus, stage: &str) -> String {
    match status {
        StageStatus::Failed { reason, .. } => reason.clone(),
        other => panic!("{stage}: expected Failed, got {other:?}"),
    }
}

fn items(analysis: &EvidenceAnalysis, modality: Modality) -> Vec<&EvidenceItem> {
    analysis
        .evidence
        .iter()
        .filter(|item| item.modality == modality)
        .collect()
}

fn coverage(analysis: &EvidenceAnalysis, modality: Modality) -> &CoverageStatus {
    &analysis
        .coverage
        .iter()
        .find(|entry| entry.modality == modality)
        .expect("coverage entry per modality")
        .status
}

fn scene_run(analysis: &EvidenceAnalysis, receipt: &ProducerReceipt) -> SceneRun {
    let mut scenes: Vec<SceneInterval> = analysis
        .evidence
        .iter()
        .filter_map(|item| match &item.content {
            EvidenceContent::Scene(scene) => Some(SceneInterval {
                id: scene.scene_id.clone(),
                index: scene.scene_index,
                start_seconds: item.start_seconds,
                end_seconds: item.end_seconds,
                start_frame: scene.start_frame,
                end_frame: scene.end_frame,
            }),
            _ => None,
        })
        .collect();
    scenes.sort_by_key(|scene| scene.index);
    SceneRun {
        receipt: receipt.clone(),
        scenes,
    }
}

fn assert_receipt(
    receipt: &ProducerReceipt,
    modality: Modality,
    producer: &str,
    producer_version: &str,
    model: &str,
) {
    assert_eq!(receipt.modality, modality);
    assert_eq!(receipt.producer, producer);
    assert_eq!(receipt.producer_version, producer_version);
    assert_eq!(receipt.model, model);
    assert_eq!(
        receipt.input_hash,
        format!("sha256:{FIXTURE_SHA256}"),
        "receipts name the excerpt's input hash"
    );
}

/// Every first-run property of one real three-stage analysis of the excerpt.
fn assert_first_run(
    report: &AnalysisReport,
    env: &ModelEnv,
    speech: &AudioAnalysisSpeechProducer,
    ocr: &VisualAnalysisOcrProducer,
) {
    let analysis = &report.analysis;
    let media = fixture_media();
    assert_eq!(analysis.media, media);
    for item in &analysis.evidence {
        assert_eq!(item.asset_id, media.asset_id);
        assert_eq!(item.content_hash.as_str(), FIXTURE_SHA256);
        assert_eq!(item.receipt.input_hash, input_hash(&media.content_hash));
        assert!(
            item.start_seconds >= 0.0 && item.start_seconds <= item.end_seconds,
            "{item:?}"
        );
    }

    // --- Scenes: the canonical producer, at least two scenes, cuts at visual changes.
    let (scene_receipt, scene_count) = completed(&report.scenes, "scenes");
    assert!(scene_count >= 2, "at least two scenes, got {scene_count}");
    assert_receipt(
        &scene_receipt,
        Modality::Scene,
        SCENE_PRODUCER,
        media_intelligence_analysis::VISUAL_ANALYSIS_REVISION,
        SCENE_MODEL,
    );
    let scenes = scene_run(analysis, &scene_receipt);
    let intervals: Vec<(f64, f64)> = scenes
        .scenes
        .iter()
        .map(|scene| (scene.start_seconds, scene.end_seconds))
        .collect();
    let boundaries = internal_boundaries(&intervals);
    for boundary in &boundaries {
        assert!(
            near_any(*boundary, &VISUAL_CHANGES),
            "scene cut at {boundary} s where the excerpt is visually static"
        );
    }
    assert!(
        TITLE_CHANGES
            .iter()
            .any(|change| near_any(*change, &boundaries)),
        "a slide title change is a scene boundary; boundaries {boundaries:?}"
    );

    // --- Speech: real Candle Whisper words, inside the excerpt.
    let (speech_receipt, speech_count) = completed(&report.speech, "speech");
    assert_receipt(
        &speech_receipt,
        Modality::Speech,
        SPEECH_PRODUCER,
        AUDIO_ANALYSIS_REVISION,
        &env.whisper_model_id,
    );
    assert_eq!(speech_receipt.config_hash, speech.identity().config_hash);
    let speech_items = items(analysis, Modality::Speech);
    assert_eq!(speech_items.len(), speech_count);
    let mut transcript = String::new();
    let mut spans = Vec::new();
    for item in &speech_items {
        let EvidenceContent::Speech(segment) = &item.content else {
            panic!("speech item with non-speech content: {item:?}");
        };
        assert!(
            item.end_seconds > item.start_seconds,
            "segment has a positive duration: {item:?}"
        );
        assert!(
            item.end_seconds <= FIXTURE_CONTAINER_SECONDS + SPEECH_END_TOLERANCE_SECONDS,
            "segment ends inside the excerpt: {item:?}"
        );
        transcript.push_str(&segment.text);
        transcript.push(' ');
        spans.push((item.start_seconds, item.end_seconds));
    }
    eprintln!("transcript: {transcript}");
    let words = tokens(&transcript).len();
    assert!(
        words >= MIN_TRANSCRIPT_WORDS,
        "real narration yields at least {MIN_TRANSCRIPT_WORDS} words, got {words}"
    );
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut covered = 0.0;
    let mut reach = 0.0_f64;
    for (start, end) in spans {
        let start = start.max(reach);
        if end > start {
            covered += end - start;
        }
        reach = reach.max(end);
    }
    assert!(
        covered >= MIN_SPEECH_COVERAGE_SECONDS,
        "segments cover {covered:.1} s of near-continuous narration"
    );
    let hits = speech_vocabulary_hits(&transcript);
    assert!(
        !hits.is_empty(),
        "the narration of these slides says at least one of {SPEECH_VOCABULARY:?}; \
         transcript: {transcript}"
    );

    // --- Visible text: real TrOCR reads of slide text, placed in the right scene.
    let (ocr_receipt, ocr_count) = completed(&report.visible_text, "visible text");
    let bound = scene_bound_identity(&ocr.identity(), &scenes);
    assert_receipt(
        &ocr_receipt,
        Modality::VisibleText,
        OCR_PRODUCER,
        VISUAL_ANALYSIS_REVISION,
        "Xenova/trocr-base-printed",
    );
    assert_eq!(ocr_receipt.config_hash, bound.config_hash);
    let ocr_items = items(analysis, Modality::VisibleText);
    assert_eq!(ocr_items.len(), ocr_count);
    let mut matched_any = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for item in &ocr_items {
        let EvidenceContent::VisibleText(text) = &item.content else {
            panic!("visible-text item with other content: {item:?}");
        };
        assert!(!text.observations.is_empty(), "{item:?}");
        assert!(
            item.end_seconds <= FIXTURE_DURATION_SECONDS + BOUNDARY_TOLERANCE_SECONDS,
            "{item:?}"
        );
        eprintln!(
            "ocr [{:.2}, {:.2}] track={:?} role={:?}: {:?}",
            item.start_seconds, item.end_seconds, text.track_id, text.role, text.text
        );
        // No repeated-OCR inflation: one item per distinct reading per scene.
        let scene = scenes
            .scenes
            .iter()
            .position(|scene| {
                item.start_seconds >= scene.start_seconds && item.start_seconds < scene.end_seconds
            })
            .expect("visible text lies inside a scene");
        assert!(
            seen.insert((scene, tokens(&text.text).join(" "))),
            "the same reading appears twice in scene {scene}: {:?}",
            text.text
        );
        for observation in &text.observations {
            for slide in slides_at(observation.timestamp_seconds) {
                for word in matched_words(&text.text, slide.words) {
                    matched_any.push((slide.name, word, observation.timestamp_seconds));
                }
            }
        }
    }
    assert!(
        !matched_any.is_empty(),
        "at least one OCR reading contains a content word of the slide on screen at its \
         time (OCR noise tolerated, see ocr_token_matches)"
    );
    eprintln!("ocr slide matches: {matched_any:?}");

    // --- Composition: per-modality coverage and cross-modal relations.
    for modality in [Modality::Speech, Modality::VisibleText, Modality::Scene] {
        assert!(
            matches!(
                coverage(analysis, modality),
                CoverageStatus::CompletedWithFindings { .. }
            ),
            "{modality:?} coverage: {:?}",
            coverage(analysis, modality)
        );
    }
    let ids: std::collections::BTreeSet<&str> = analysis
        .evidence
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    for relation in &analysis.relations {
        assert!(ids.contains(relation.subject.as_str()), "{relation:?}");
        assert!(ids.contains(relation.object.as_str()), "{relation:?}");
    }
    for kind in [
        RelationKind::SceneSpeech,
        RelationKind::SceneVisibleText,
        RelationKind::SpeechVisibleTextCooccurrence,
    ] {
        assert!(
            analysis
                .relations
                .iter()
                .any(|relation| relation.kind == kind),
            "at least one {kind:?} relation"
        );
    }
}

fn evidence_of(analysis: &EvidenceAnalysis, modality: Modality) -> Vec<EvidenceItem> {
    items(analysis, modality).into_iter().cloned().collect()
}

/// Real run, unchanged rerun (all Reused, identical analysis), OCR-request change (only
/// OCR re-runs), and a missing TrOCR bundle (only OCR fails).
#[test]
fn model_backed_analysis_of_the_excerpt_with_reuse_and_partial_invalidation() {
    let Some(env) =
        model_env("model_backed_analysis_of_the_excerpt_with_reuse_and_partial_invalidation")
    else {
        return;
    };
    let _guard = MODEL_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let speech = speech(&env);
    let ocr = ocr(&env);
    let all = || profile(true, true, true);
    let mut cache = InMemoryStageCache::new();

    // 1. Real run.
    let first = run(&speech, &ocr, all(), &mut cache, &CancellationToken::new());
    eprintln!(
        "report JSON:\n{}",
        serde_json::to_string_pretty(&first).expect("report serializes")
    );
    assert_first_run(&first, &env, &speech, &ocr);
    assert_eq!(cache.len(), 3, "one cached run per completed stage");

    // 2. Unchanged rerun with the same cache: every stage Reused, identical analysis.
    let second = run(&speech, &ocr, all(), &mut cache, &CancellationToken::new());
    for (modality, earlier, later) in [
        (Modality::Speech, &first.speech, &second.speech),
        (
            Modality::VisibleText,
            &first.visible_text,
            &second.visible_text,
        ),
        (Modality::Scene, &first.scenes, &second.scenes),
    ] {
        let (receipt, count) = completed(earlier, &format!("{modality:?}"));
        assert_eq!(
            reused(later, &format!("{modality:?} rerun")),
            (receipt, count),
            "{modality:?} reuses the first run"
        );
    }
    assert_eq!(
        second.analysis, first.analysis,
        "rerun analysis is identical"
    );

    // 3. Only the OCR request changes: OCR re-runs, speech and scenes are reused unchanged.
    let changed_ocr = ocr_with(&env, env.trocr_bundle_root.clone(), changed_request());
    assert_ne!(
        changed_ocr.identity().config_hash,
        ocr.identity().config_hash
    );
    let third = run(
        &speech,
        &changed_ocr,
        all(),
        &mut cache,
        &CancellationToken::new(),
    );
    reused(&third.speech, "speech after OCR change");
    reused(&third.scenes, "scenes after OCR change");
    let changed_receipt = match &third.visible_text {
        StageStatus::CompletedWithFindings { receipt, .. }
        | StageStatus::CompletedEmpty { receipt } => receipt.clone(),
        other => panic!("visible text re-runs after its request changed, got {other:?}"),
    };
    let (first_ocr_receipt, _) = completed(&first.visible_text, "visible text");
    assert_ne!(changed_receipt.config_hash, first_ocr_receipt.config_hash);
    assert_eq!(changed_receipt.producer, OCR_PRODUCER);
    for modality in [Modality::Speech, Modality::Scene] {
        assert_eq!(
            evidence_of(&third.analysis, modality),
            evidence_of(&first.analysis, modality),
            "{modality:?} evidence is untouched by an OCR change"
        );
    }
    assert_eq!(
        cache.len(),
        4,
        "the changed OCR run is cached beside the first"
    );

    // 4. Missing TrOCR bundle: OCR fails with a reason naming it; the others still complete.
    let missing = env.trocr_bundle_root.join("does-not-exist-issue-34");
    let broken_ocr = ocr_with(&env, missing.clone(), OcrRequest::default());
    let fourth = run(
        &speech,
        &broken_ocr,
        all(),
        &mut cache,
        &CancellationToken::new(),
    );
    let reason = failed_reason(&fourth.visible_text, "visible text without a TrOCR bundle");
    assert!(
        reason.contains("bundle_root") && reason.contains(&missing.display().to_string()),
        "actionable reason: {reason}"
    );
    reused(&fourth.speech, "speech beside a failed OCR stage");
    reused(&fourth.scenes, "scenes beside a failed OCR stage");
    assert!(items(&fourth.analysis, Modality::VisibleText).is_empty());
    assert!(matches!(
        coverage(&fourth.analysis, Modality::VisibleText),
        CoverageStatus::Failed { .. }
    ));
    assert_eq!(
        evidence_of(&fourth.analysis, Modality::Speech),
        evidence_of(&first.analysis, Modality::Speech)
    );
    assert_eq!(cache.len(), 4, "failures are not cached");
}

/// Missing Whisper bundle: speech fails with a reason naming the path, while real scenes
/// and real OCR complete in the same analysis (fresh cache, so they actually run).
#[test]
fn missing_whisper_model_fails_speech_while_scenes_and_ocr_complete() {
    let Some(env) = model_env("missing_whisper_model_fails_speech_while_scenes_and_ocr_complete")
    else {
        return;
    };
    let _guard = MODEL_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let missing = env.whisper_bundle.join("does-not-exist-issue-34");
    let speech = speech_with_bundle(&env, missing.clone());
    let ocr = ocr(&env);
    let mut cache = InMemoryStageCache::new();
    let report = run(
        &speech,
        &ocr,
        profile(true, true, true),
        &mut cache,
        &CancellationToken::new(),
    );
    let reason = failed_reason(&report.speech, "speech without a Whisper bundle");
    assert!(
        reason.contains(&missing.display().to_string()),
        "actionable reason names the missing location: {reason}"
    );
    assert!(items(&report.analysis, Modality::Speech).is_empty());
    assert!(matches!(
        coverage(&report.analysis, Modality::Speech),
        CoverageStatus::Failed { .. }
    ));
    completed(&report.scenes, "scenes beside a failed speech stage");
    completed(&report.visible_text, "OCR beside a failed speech stage");
    assert!(
        cache
            .keys()
            .iter()
            .all(|key| key.modality != Modality::Speech),
        "a failed speech run is not cached"
    );
    assert!(!report
        .analysis
        .relations
        .iter()
        .any(|relation| relation.kind == RelationKind::SpeechVisibleTextCooccurrence));
}

/// Speech not requested (its producer supplied): NotRequested, no speech evidence or
/// speech relations, while scenes and real OCR complete.
#[test]
fn speech_not_requested_produces_no_speech_evidence() {
    let Some(env) = model_env("speech_not_requested_produces_no_speech_evidence") else {
        return;
    };
    let _guard = MODEL_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let speech = speech(&env);
    let ocr = ocr(&env);
    let mut cache = InMemoryStageCache::new();
    let report = run(
        &speech,
        &ocr,
        profile(false, true, true),
        &mut cache,
        &CancellationToken::new(),
    );
    assert_eq!(report.speech, StageStatus::NotRequested);
    assert_eq!(
        *coverage(&report.analysis, Modality::Speech),
        CoverageStatus::NotRequested
    );
    assert!(items(&report.analysis, Modality::Speech).is_empty());
    assert!(!report.analysis.relations.iter().any(|relation| matches!(
        relation.kind,
        RelationKind::SceneSpeech | RelationKind::SpeechVisibleTextCooccurrence
    )));
    completed(&report.scenes, "scenes");
    completed(&report.visible_text, "visible text");
    assert!(cache
        .keys()
        .iter()
        .all(|key| key.modality != Modality::Speech));
}

/// Cancelled before the call: every requested stage Cancelled, no evidence, nothing
/// cached, and no model is loaded (the adapters check cancellation first).
#[test]
fn pre_cancelled_analysis_runs_no_model_and_reports_cancelled() {
    let Some(env) = model_env("pre_cancelled_analysis_runs_no_model_and_reports_cancelled") else {
        return;
    };
    let speech = speech(&env);
    let ocr = ocr(&env);
    let mut cache = InMemoryStageCache::new();
    let token = CancellationToken::new();
    token.cancel();
    let report = run(&speech, &ocr, profile(true, true, true), &mut cache, &token);
    assert_eq!(report.speech, StageStatus::Cancelled);
    assert_eq!(report.visible_text, StageStatus::Cancelled);
    assert_eq!(report.scenes, StageStatus::Cancelled);
    assert!(report.analysis.evidence.is_empty());
    assert!(report.analysis.relations.is_empty());
    assert!(cache.is_empty());
}
