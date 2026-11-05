//! Implementation of [`crate::analyze`]: media integrity, stage selection, cache reuse,
//! producer execution, receipt checks and composition.

use std::cell::Cell;
use std::fs::{File, Metadata};
use std::io::Read;
use std::path::Path;

use media_intelligence_core::exact_timing::{
    ExactAnchorTimingV1, ExactAnchorV1, ExactTimingSidecarV1,
};
use media_intelligence_core::{
    compose, CompositionInput, ModalityInput, SceneInterval, SceneRun, SpeechRun, VisibleTextRun,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use video_analysis_ffmpeg::{FfmpegSourceOptions, FfmpegVideoSource};

use crate::{
    input_hash, scene_bound_identity, AnalysisError, AnalysisReport, AnalysisRequest, CachedStage,
    CancellationToken, ContentHash, ExactSceneAnalysisReport, MediaIdentity, Modality,
    ProducerError, ProducerIdentity, ProducerInput, ProducerReceipt, Producers, SceneConfig,
    StageCache, StageKey, StageStatus, VisibleTextProducer, VISUAL_ANALYSIS_REVISION,
};

pub(crate) fn sha256_prefixed(bytes: &[u8]) -> String {
    format!("sha256:{}", hex(&Sha256::digest(bytes)))
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// The media file as verified at the start of the operation.
///
/// Metadata (size, modification time) is never taken as proof of unchanged bytes: every
/// check re-hashes. To bound the cost, the bytes are re-hashed only after a producer has
/// had access to the file since the last successful check (`dirty`), so an operation hashes
/// the file once up front plus once after each executed producer.
struct VerifiedMedia<'a> {
    path: &'a Path,
    declared: &'a ContentHash,
    dirty: Cell<bool>,
}

impl VerifiedMedia<'_> {
    /// Whether the bytes still hash to the declared content hash.
    fn unchanged(&self) -> Result<bool, String> {
        if !self.dirty.get() {
            return Ok(true);
        }
        let actual = hash_file(self.path).map_err(|error| error.to_string())?;
        let unchanged = &actual == self.declared;
        self.dirty.set(!unchanged);
        Ok(unchanged)
    }

    /// A producer is about to get (or just had) access to the file.
    fn touched(&self) {
        self.dirty.set(true);
    }
}

fn hash_file(path: &Path) -> std::io::Result<ContentHash> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1 << 16];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let hex = hex(&hasher.finalize());
    Ok(ContentHash::parse(hex).expect("sha256 hex is a valid content hash"))
}

fn regular_file_metadata(path: &Path) -> Option<Metadata> {
    std::fs::metadata(path).ok().filter(Metadata::is_file)
}

/// Outcome of one requested stage before evidence is counted.
// Short-lived, one value per modality per call; boxing would only add noise.
#[allow(clippy::large_enum_variant)]
enum Stage<R> {
    NotRequested,
    Unavailable,
    Cancelled,
    Failed {
        receipt: Option<ProducerReceipt>,
        reason: String,
    },
    Completed {
        run: R,
        reused: bool,
    },
}

#[derive(Clone)]
struct SceneStageRun {
    run: SceneRun,
    original_pts: Option<Vec<ExactAnchorV1>>,
}

#[derive(Clone)]
struct VisibleTextStageRun {
    run: VisibleTextRun,
    original_pts: Option<Vec<ExactAnchorV1>>,
}

fn scene_input(stage: &Stage<SceneStageRun>) -> ModalityInput<SceneRun> {
    // Only clone the legacy run: original PTS can be large and is handed off
    // separately, never copied into the seconds-only composition payload.
    match stage {
        Stage::NotRequested => ModalityInput::NotRequested,
        Stage::Unavailable | Stage::Cancelled => ModalityInput::Unprocessed,
        Stage::Failed { receipt, reason } => ModalityInput::Failed {
            receipt: receipt.clone(),
            reason: reason.clone(),
        },
        Stage::Completed { run, .. } => ModalityInput::Completed {
            runs: vec![run.run.clone()],
        },
    }
}

fn visible_text_input(stage: &Stage<VisibleTextStageRun>) -> ModalityInput<VisibleTextRun> {
    match stage {
        Stage::NotRequested => ModalityInput::NotRequested,
        Stage::Unavailable | Stage::Cancelled => ModalityInput::Unprocessed,
        Stage::Failed { receipt, reason } => ModalityInput::Failed {
            receipt: receipt.clone(),
            reason: reason.clone(),
        },
        Stage::Completed { run, .. } => ModalityInput::Completed {
            runs: vec![run.run.clone()],
        },
    }
}

impl<R: Clone> Stage<R> {
    fn input(&self) -> ModalityInput<R> {
        match self {
            Stage::NotRequested => ModalityInput::NotRequested,
            Stage::Unavailable | Stage::Cancelled => ModalityInput::Unprocessed,
            Stage::Failed { receipt, reason } => ModalityInput::Failed {
                receipt: receipt.clone(),
                reason: reason.clone(),
            },
            Stage::Completed { run, .. } => ModalityInput::Completed {
                runs: vec![run.clone()],
            },
        }
    }

    fn status(&self, receipt: impl Fn(&R) -> &ProducerReceipt, count: usize) -> StageStatus {
        match self {
            Stage::NotRequested => StageStatus::NotRequested,
            Stage::Unavailable => StageStatus::Unavailable,
            Stage::Cancelled => StageStatus::Cancelled,
            Stage::Failed { receipt, reason } => StageStatus::Failed {
                receipt: receipt.clone(),
                reason: reason.clone(),
            },
            Stage::Completed { run, reused: true } => StageStatus::Reused {
                receipt: receipt(run).clone(),
                evidence_count: count,
            },
            Stage::Completed { run, .. } if count == 0 => StageStatus::CompletedEmpty {
                receipt: receipt(run).clone(),
            },
            Stage::Completed { run, .. } => StageStatus::CompletedWithFindings {
                receipt: receipt(run).clone(),
                evidence_count: count,
            },
        }
    }
}

/// Everything a stage needs besides its producer.
struct Context<'a> {
    media: &'a MediaIdentity,
    verified: &'a VerifiedMedia<'a>,
    input_hash: &'a str,
    cancellation: &'a CancellationToken,
}

pub(crate) fn analyze(
    request: &AnalysisRequest,
    producers: &Producers<'_>,
    cache: &mut dyn StageCache,
    cancellation: &CancellationToken,
) -> Result<AnalysisReport, AnalysisError> {
    analyze_internal(request, producers, cache, cancellation, false).map(|result| result.legacy)
}

pub(crate) fn analyze_with_exact_scene_timing(
    request: &AnalysisRequest,
    producers: &Producers<'_>,
    cache: &mut dyn StageCache,
    cancellation: &CancellationToken,
) -> Result<ExactSceneAnalysisReport, AnalysisError> {
    analyze_internal(request, producers, cache, cancellation, true)
}

fn analyze_internal(
    request: &AnalysisRequest,
    producers: &Producers<'_>,
    cache: &mut dyn StageCache,
    cancellation: &CancellationToken,
    original_pts: bool,
) -> Result<ExactSceneAnalysisReport, AnalysisError> {
    let path = request.media_path.as_path();
    if regular_file_metadata(path).is_none() {
        return Err(AnalysisError::MediaMissing {
            path: request.media_path.clone(),
        });
    }
    if let Some(config) = &request.profile.scenes {
        config.validate()?;
    }
    let declared = &request.media.content_hash;
    let actual = hash_file(path).map_err(|source| AnalysisError::Io {
        path: request.media_path.clone(),
        source,
    })?;
    if &actual != declared {
        return Err(AnalysisError::ContentHashMismatch {
            declared: declared.clone(),
            actual,
        });
    }
    let verified = VerifiedMedia {
        path,
        declared,
        dirty: Cell::new(false),
    };
    let input_hash = input_hash(declared);
    let context = Context {
        media: &request.media,
        verified: &verified,
        input_hash: &input_hash,
        cancellation,
    };

    let speech = match (request.profile.speech, producers.speech) {
        (false, _) => Stage::NotRequested,
        (true, None) => stage_without_producer(cancellation),
        (true, Some(producer)) => run_stage(
            &context,
            cache,
            Modality::Speech,
            producer.identity(),
            |run: &SpeechRun| &run.receipt,
            CachedStage::Speech,
            |cached| match cached {
                CachedStage::Speech(run) => Some(run),
                _ => None,
            },
            None,
            None,
            |input| producer.run(input),
        ),
    };
    let scenes = match &request.profile.scenes {
        None => Stage::NotRequested,
        Some(config) => run_stage(
            &context,
            cache,
            Modality::Scene,
            if original_pts {
                config.exact_producer_identity()
            } else {
                config.producer_identity()
            },
            |result: &SceneStageRun| &result.run.receipt,
            |result| match result.original_pts {
                Some(anchors) => CachedStage::ExactScenes {
                    run: result.run,
                    anchors,
                },
                None => CachedStage::Scenes(result.run),
            },
            |cached| match (original_pts, cached) {
                (true, CachedStage::ExactScenes { run, anchors }) => Some(SceneStageRun {
                    run,
                    original_pts: Some(anchors),
                }),
                (false, CachedStage::Scenes(run)) => Some(SceneStageRun {
                    run,
                    original_pts: None,
                }),
                _ => None,
            },
            None,
            None,
            |input| detect_scenes(input, config, original_pts),
        ),
    };

    // Visible text runs after scenes so that a scene-dependent producer (#33) can sample
    // the scene intervals of this same analysis.
    let scene_run = match &scenes {
        Stage::Completed { run, .. } => Some(&run.run),
        _ => None,
    };
    let visible_text = match (request.profile.visible_text, producers.visible_text) {
        (false, _) => Stage::NotRequested,
        (true, None) => stage_without_producer(cancellation),
        (true, Some(producer)) if producer.requires_scenes() && !cancellation.is_cancelled() => {
            match scene_run {
                None => Stage::Failed {
                    receipt: None,
                    reason: scenes_missing_reason(request.profile.scenes.is_some()),
                },
                Some(scene_run) => run_visible_text_stage(
                    &context,
                    cache,
                    producer,
                    scene_bound_identity(&producer.identity(), scene_run),
                    Some(scene_run),
                    Some(scene_run),
                    original_pts,
                ),
            }
        }
        (true, Some(producer)) => run_visible_text_stage(
            &context,
            cache,
            producer,
            producer.identity(),
            scene_run,
            None,
            original_pts,
        ),
    };
    let composition = CompositionInput {
        media: request.media.clone(),
        speech: speech.input(),
        visible_text: visible_text_input(&visible_text),
        scenes: scene_input(&scenes),
    };
    let analysis = compose(&composition).map_err(AnalysisError::Composition)?;
    let mut exact_modalities = Vec::new();
    let mut original_anchors = Vec::new();
    if let Stage::Completed {
        run: SceneStageRun {
            original_pts: Some(anchors),
            ..
        },
        ..
    } = &scenes
    {
        if !anchors.is_empty() {
            exact_modalities.push(Modality::Scene);
            original_anchors.extend(anchors.iter().cloned());
        }
    }
    if let Stage::Completed {
        run: VisibleTextStageRun {
            original_pts: Some(anchors),
            ..
        },
        ..
    } = &visible_text
    {
        if !anchors.is_empty() {
            exact_modalities.push(Modality::VisibleText);
            original_anchors.extend(anchors.iter().cloned());
        }
    }
    let exact_timing = if original_anchors.is_empty() {
        None
    } else {
        let sidecar = ExactTimingSidecarV1::new(
            request.media.clone(),
            None,
            exact_modalities,
            original_anchors,
        )
        .map_err(AnalysisError::ExactTiming)?;
        sidecar
            .validate_for(&composition)
            .map_err(AnalysisError::ExactTiming)?;
        Some(sidecar)
    };
    let count = |modality: Modality| {
        analysis
            .evidence
            .iter()
            .filter(|item| item.modality == modality)
            .count()
    };

    Ok(ExactSceneAnalysisReport {
        legacy: AnalysisReport {
            speech: speech.status(|run| &run.receipt, count(Modality::Speech)),
            visible_text: visible_text.status(|run| &run.run.receipt, count(Modality::VisibleText)),
            scenes: scenes.status(|run| &run.run.receipt, count(Modality::Scene)),
            analysis,
        },
        exact_timing,
    })
}

fn scenes_missing_reason(scenes_requested: bool) -> String {
    if scenes_requested {
        "the visible-text producer samples the scene intervals of this analysis, but the \
         scene stage did not complete; fix the scene stage (see its status) and rerun"
            .to_string()
    } else {
        "the visible-text producer samples the scene intervals of this analysis, but scenes \
         were not requested; add a scene configuration to the analysis profile"
            .to_string()
    }
}

fn stage_without_producer<R>(cancellation: &CancellationToken) -> Stage<R> {
    if cancellation.is_cancelled() {
        Stage::Cancelled
    } else {
        Stage::Unavailable
    }
}

fn run_visible_text_stage(
    context: &Context<'_>,
    cache: &mut dyn StageCache,
    producer: &dyn VisibleTextProducer,
    identity: ProducerIdentity,
    scenes: Option<&SceneRun>,
    required_scenes: Option<&SceneRun>,
    original_pts: bool,
) -> Stage<VisibleTextStageRun> {
    let exact = original_pts && producer.supports_exact_frame_pts();
    run_stage(
        context,
        cache,
        Modality::VisibleText,
        identity,
        |result: &VisibleTextStageRun| &result.run.receipt,
        |result| match result.original_pts {
            Some(anchors) => CachedStage::ExactVisibleText {
                run: result.run,
                anchors,
            },
            None => CachedStage::VisibleText(result.run),
        },
        |cached| match (exact, cached) {
            (true, CachedStage::ExactVisibleText { run, anchors }) => Some(VisibleTextStageRun {
                run,
                original_pts: Some(anchors),
            }),
            (false, CachedStage::VisibleText(run)) => Some(VisibleTextStageRun {
                run,
                original_pts: None,
            }),
            _ => None,
        },
        scenes,
        required_scenes,
        |input| {
            if exact {
                producer
                    .run_with_exact_frame_pts(input)
                    .map(|(run, anchors)| VisibleTextStageRun {
                        run,
                        original_pts: Some(anchors),
                    })
            } else {
                producer.run(input).map(|run| VisibleTextStageRun {
                    run,
                    original_pts: None,
                })
            }
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn run_stage<R: Clone>(
    context: &Context<'_>,
    cache: &mut dyn StageCache,
    modality: Modality,
    identity: ProducerIdentity,
    receipt_of: impl Fn(&R) -> &ProducerReceipt,
    wrap: impl Fn(R) -> CachedStage,
    unwrap: impl Fn(CachedStage) -> Option<R>,
    scenes: Option<&SceneRun>,
    required_scenes: Option<&SceneRun>,
    execute: impl FnOnce(&ProducerInput<'_>) -> Result<R, ProducerError>,
) -> Stage<R> {
    if context.cancellation.is_cancelled() {
        return Stage::Cancelled;
    }
    let key = StageKey {
        modality,
        content_hash: context.media.content_hash.clone(),
        producer: identity.clone(),
    };
    let attempted = attempted_receipt(modality, &identity, context.input_hash);
    let changed = |when: &str| -> Option<Stage<R>> {
        match context.verified.unchanged() {
            Ok(true) => None,
            Ok(false) => Some(Stage::Failed {
                receipt: Some(attempted.clone()),
                reason: format!(
                    "media bytes no longer match the declared content hash {when} the stage"
                ),
            }),
            Err(error) => Some(Stage::Failed {
                receipt: Some(attempted.clone()),
                reason: format!("media file could not be checked {when} the stage: {error}"),
            }),
        }
    };
    // Also guards cache reuse: a cached run describes the declared bytes only while the
    // file still holds them.
    if let Some(failed) = changed("before") {
        return failed;
    }
    // A caller-owned cache entry is reused only if it passes the same checks as a fresh
    // run; otherwise it is treated as a miss and the producer runs again.
    if let Some(run) = cache.get(&key).and_then(&unwrap) {
        if validate_run(
            context,
            modality,
            &identity,
            &receipt_of,
            &wrap,
            &run,
            required_scenes,
        )
        .is_ok()
        {
            return Stage::Completed { run, reused: true };
        }
    }

    let input = ProducerInput {
        media: context.media,
        media_path: context.verified.path,
        input_hash: context.input_hash,
        cancellation: context.cancellation,
        scenes,
    };
    context.verified.touched();
    let outcome = execute(&input);
    if let Some(failed) = changed("after") {
        return failed;
    }
    let run = match outcome {
        Ok(run) => run,
        Err(ProducerError::Cancelled) => return Stage::Cancelled,
        Err(ProducerError::Failed { reason }) => {
            return Stage::Failed {
                receipt: Some(attempted),
                reason,
            }
        }
    };
    if let Err(reason) = validate_run(
        context,
        modality,
        &identity,
        &receipt_of,
        &wrap,
        &run,
        required_scenes,
    ) {
        // The attempted receipt always names the right modality, so the failure itself can
        // still be composed alongside the other stages.
        return Stage::Failed {
            receipt: Some(attempted),
            reason,
        };
    }

    cache.put(key, wrap(run.clone()));
    Stage::Completed { run, reused: false }
}

/// Receipt identity and composability checks shared by fresh and cached runs, so one bad
/// run fails only its own stage instead of the whole operation.
fn validate_run<R: Clone>(
    context: &Context<'_>,
    modality: Modality,
    identity: &ProducerIdentity,
    receipt_of: &impl Fn(&R) -> &ProducerReceipt,
    wrap: &impl Fn(R) -> CachedStage,
    run: &R,
    required_scenes: Option<&SceneRun>,
) -> Result<(), String> {
    if let Some(reason) = receipt_mismatch(receipt_of(run), modality, identity, context.input_hash)
    {
        return Err(reason);
    }
    let cached = wrap(run.clone());
    if let Some(scenes) = required_scenes {
        let text = match &cached {
            CachedStage::VisibleText(run) | CachedStage::ExactVisibleText { run, .. } => Some(run),
            _ => None,
        };
        if let Some(reason) = text.and_then(|run| observation_outside_scenes(run, scenes)) {
            return Err(reason);
        }
    }
    let input = single_stage_input(context.media, modality, &cached);
    compose(&input).map_err(|error| format!("producer run is not composable: {error}"))?;
    if let CachedStage::ExactScenes { run, anchors } = &cached {
        if run.scenes.is_empty() {
            return if anchors.is_empty() {
                Ok(())
            } else {
                Err("empty scene run claims original PTS anchors".into())
            };
        }
        let Some(first) = run.scenes.first() else {
            return Err("exact scene run is empty".into());
        };
        let has_media_origin = anchors.iter().any(|anchor| {
            anchor.record_id == first.id
                && matches!(
                    anchor.timing,
                    ExactAnchorTimingV1::Range { start, .. } if start.pts == 0
                )
        });
        if !has_media_origin {
            return Err("exact scene clock has no verified media-relative zero origin".into());
        }
        ExactTimingSidecarV1::new(
            context.media.clone(),
            None,
            vec![Modality::Scene],
            anchors.clone(),
        )
        .and_then(|sidecar| sidecar.validate_for(&input))
        .map_err(|error| format!("cached exact PTS is invalid: {error}"))?;
    }
    if let CachedStage::ExactVisibleText { run, anchors } = &cached {
        if run.observations.is_empty() {
            return if anchors.is_empty() {
                Ok(())
            } else {
                Err("empty OCR run claims original PTS anchors".into())
            };
        }
        ExactTimingSidecarV1::new(
            context.media.clone(),
            None,
            vec![Modality::VisibleText],
            anchors.clone(),
        )
        .map_err(|error| error.to_string())?
        .validate_for(&input)
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// A scene-dependent run claims its frames were sampled from `scenes`; reject one (fresh
/// or cached) with an observation outside every scene interval.
fn observation_outside_scenes(run: &VisibleTextRun, scenes: &SceneRun) -> Option<String> {
    run.observations.iter().find_map(|observation| {
        // Composition places evidence by time, so the timestamp must lie in the half-open
        // scene interval; a frame index, when both sides have one, must agree.
        let inside = scenes.scenes.iter().any(|scene| {
            let in_time = scene.start_seconds <= observation.timestamp_seconds
                && observation.timestamp_seconds < scene.end_seconds;
            let in_frames = match (observation.frame_index, scene.start_frame, scene.end_frame) {
                (Some(frame), Some(start), Some(end)) => start <= frame && frame < end,
                _ => true,
            };
            in_time && in_frames
        });
        (!inside).then(|| {
            format!(
                "visible-text observation `{}` lies outside the scene intervals it was \
                 sampled from",
                observation.id
            )
        })
    })
}

fn single_stage_input(
    media: &MediaIdentity,
    modality: Modality,
    stage: &CachedStage,
) -> CompositionInput {
    let mut input = CompositionInput {
        media: media.clone(),
        speech: ModalityInput::NotRequested,
        visible_text: ModalityInput::NotRequested,
        scenes: ModalityInput::NotRequested,
    };
    match (modality, stage) {
        (Modality::Speech, CachedStage::Speech(run)) => {
            input.speech = ModalityInput::Completed {
                runs: vec![run.clone()],
            }
        }
        (Modality::VisibleText, CachedStage::VisibleText(run))
        | (Modality::VisibleText, CachedStage::ExactVisibleText { run, .. }) => {
            input.visible_text = ModalityInput::Completed {
                runs: vec![run.clone()],
            }
        }
        (Modality::Scene, CachedStage::Scenes(run))
        | (Modality::Scene, CachedStage::ExactScenes { run, .. }) => {
            input.scenes = ModalityInput::Completed {
                runs: vec![run.clone()],
            }
        }
        _ => unreachable!("stage variant always matches its modality"),
    }
    input
}

fn attempted_receipt(
    modality: Modality,
    identity: &ProducerIdentity,
    input_hash: &str,
) -> ProducerReceipt {
    ProducerReceipt {
        modality,
        producer: identity.producer.clone(),
        producer_version: identity.producer_version.clone(),
        model: identity.model.clone(),
        model_version: identity.model_version.clone(),
        config_hash: identity.config_hash.clone(),
        input_hash: input_hash.to_string(),
        run_id: None,
        config: None,
    }
}

fn receipt_mismatch(
    receipt: &ProducerReceipt,
    modality: Modality,
    identity: &ProducerIdentity,
    input_hash: &str,
) -> Option<String> {
    if receipt.modality != modality {
        return Some(format!(
            "run receipt names modality {:?}, expected {modality:?}",
            receipt.modality
        ));
    }
    let named = ProducerIdentity {
        producer: receipt.producer.clone(),
        producer_version: receipt.producer_version.clone(),
        model: receipt.model.clone(),
        model_version: receipt.model_version.clone(),
        config_hash: receipt.config_hash.clone(),
    };
    if &named != identity {
        return Some(format!(
            "run receipt names producer {named:?}, but the producer declared {identity:?}"
        ));
    }
    if receipt.input_hash != input_hash {
        return Some(format!(
            "run receipt names input {}, but the analysed media is {input_hash}",
            receipt.input_hash
        ));
    }
    None
}

/// Canonical content-scene detection through visual-analysis.
fn detect_scenes(
    input: &ProducerInput<'_>,
    config: &SceneConfig,
    original_pts: bool,
) -> Result<SceneStageRun, ProducerError> {
    let failed = |stage: &str, error: &dyn std::fmt::Display| ProducerError::Failed {
        reason: format!("visual-analysis {stage} failed: {error}"),
    };
    let mut source = if original_pts {
        FfmpegVideoSource::open_path_with_exact_frame_pts(
            input.media_path,
            FfmpegSourceOptions::recorded(),
        )
    } else {
        FfmpegVideoSource::open(input.media_path)
    }
    .map_err(|error| failed("video open", &error))?;
    let result = video_analysis_ingest::surface::detect_content_scenes(
        &mut source,
        config.threshold,
        config.min_scene_len,
    )
    .map_err(|error| failed("scene detection", &error))?;
    drop(source);

    if original_pts
        && result
            .scenes
            .first()
            .is_some_and(|scene| scene.start.timestamp.pts != 0)
    {
        return Err(ProducerError::Failed {
            reason: "source first-frame PTS is nonzero; an exact media-relative clock mapping is required"
                .into(),
        });
    }

    let scenes = result
        .scenes
        .iter()
        .enumerate()
        .map(|(index, scene)| SceneInterval {
            id: format!("scene-{index}"),
            index: index as u64,
            start_seconds: scene.start.timestamp.seconds(),
            end_seconds: scene.end.timestamp.seconds(),
            start_frame: Some(scene.start.frame_index),
            end_frame: Some(scene.end.frame_index),
        })
        .collect();
    let identity = if original_pts {
        config.exact_producer_identity()
    } else {
        config.producer_identity()
    };
    let mut recorded_config = json!({
        "adapter": "video-analysis-ingest.detect_content_scenes",
        "threshold": config.threshold,
        "minSceneLen": config.min_scene_len,
        "visualAnalysisRevision": VISUAL_ANALYSIS_REVISION,
    });
    if original_pts {
        recorded_config["sourcePts"] = json!("ffprobe-frame-v1");
    }
    let receipt = ProducerReceipt {
        config: Some(recorded_config),
        ..attempted_receipt(Modality::Scene, &identity, input.input_hash)
    };
    let anchors = if original_pts {
        Some(
            result
                .scenes
                .iter()
                .enumerate()
                .map(|(index, scene)| -> Result<ExactAnchorV1, ProducerError> {
                    // Visual Analysis may resolve Foundation's compatible media
                    // crate from a different source instance in a consumer graph.
                    // Transfer PTS/timebase integers directly, never via seconds.
                    let exact = |timestamp_pts: i64, timebase_num: i32, timebase_den: i32| {
                        let base = media_core::Timebase::try_new(timebase_num, timebase_den)
                            .map_err(|error| failed("original timebase", &error))?;
                        media_core::Timestamp::try_new(timestamp_pts, base)
                            .map_err(|error| failed("original PTS", &error))
                    };
                    let start = scene.start.timestamp;
                    let end = scene.end.timestamp;
                    Ok(ExactAnchorV1 {
                        receipt: receipt.clone(),
                        record_id: format!("scene-{index}"),
                        timing: ExactAnchorTimingV1::Range {
                            start: exact(start.pts, start.timebase.num, start.timebase.den)?,
                            end: exact(end.pts, end.timebase.num, end.timebase.den)?,
                        },
                        frame_index: None,
                        start_frame: Some(scene.start.frame_index),
                        end_frame: Some(scene.end.frame_index),
                    })
                })
                .collect::<Result<Vec<_>, ProducerError>>()?,
        )
    } else {
        None
    };
    Ok(SceneStageRun {
        run: SceneRun { receipt, scenes },
        original_pts: anchors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use media_intelligence_core::OcrObservation;

    fn receipt(modality: Modality) -> ProducerReceipt {
        ProducerReceipt {
            modality,
            producer: "p".to_string(),
            producer_version: "1".to_string(),
            model: "m".to_string(),
            model_version: "1".to_string(),
            config_hash: "c".to_string(),
            input_hash: "i".to_string(),
            run_id: None,
            config: None,
        }
    }

    fn check(frame: Option<u64>, seconds: f64) -> Option<String> {
        let scenes = SceneRun {
            receipt: receipt(Modality::Scene),
            scenes: vec![SceneInterval {
                id: "s0".to_string(),
                index: 0,
                start_seconds: 0.0,
                end_seconds: 4.0,
                start_frame: Some(0),
                end_frame: Some(100),
            }],
        };
        let run = VisibleTextRun {
            receipt: receipt(Modality::VisibleText),
            observations: vec![OcrObservation {
                id: "o".to_string(),
                text: "t".to_string(),
                timestamp_seconds: seconds,
                frame_index: frame,
                language: None,
                region: None,
                confidence: None,
            }],
            tracks: vec![],
        };
        observation_outside_scenes(&run, &scenes)
    }

    #[test]
    fn scene_membership_needs_time_and_frame_inside_the_half_open_interval() {
        assert!(check(Some(50), 2.0).is_none());
        assert!(check(None, 2.0).is_none());
        assert!(
            check(Some(50), 9.0).is_some(),
            "stale timestamp with an in-range frame"
        );
        assert!(check(Some(150), 2.0).is_some(), "frame outside");
        assert!(
            check(None, 4.0).is_some(),
            "exactly at the scene end is outside"
        );
    }
}
