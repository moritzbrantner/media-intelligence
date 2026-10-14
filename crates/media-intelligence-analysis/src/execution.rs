//! Implementation of [`crate::analyze`]: media integrity, stage selection, cache reuse,
//! producer execution, receipt checks and composition.

use std::fs::{File, Metadata};
use std::io::Read;
use std::path::Path;
use std::time::SystemTime;

use media_intelligence_core::{
    compose, CompositionInput, ModalityInput, SceneInterval, SceneRun, SpeechRun, VisibleTextRun,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use video_analysis_ffmpeg::FfmpegVideoSource;

use crate::{
    input_hash, AnalysisError, AnalysisReport, AnalysisRequest, CachedStage, CancellationToken,
    ContentHash, MediaIdentity, Modality, ProducerError, ProducerIdentity, ProducerInput,
    ProducerReceipt, Producers, SceneConfig, StageCache, StageKey, StageStatus,
    VISUAL_ANALYSIS_REVISION,
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
struct VerifiedMedia<'a> {
    path: &'a Path,
    declared: &'a ContentHash,
    len: u64,
    modified: Option<SystemTime>,
}

impl VerifiedMedia<'_> {
    /// Whether the bytes still hash to the declared content hash. Unchanged size and
    /// modification time are taken as unchanged bytes; anything else is re-hashed.
    fn unchanged(&self) -> Result<bool, String> {
        let metadata = std::fs::metadata(self.path).map_err(|error| error.to_string())?;
        if metadata.len() == self.len && metadata.modified().ok() == self.modified {
            return Ok(true);
        }
        let actual = hash_file(self.path).map_err(|error| error.to_string())?;
        Ok(&actual == self.declared)
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
    let path = request.media_path.as_path();
    let Some(metadata) = regular_file_metadata(path) else {
        return Err(AnalysisError::MediaMissing {
            path: request.media_path.clone(),
        });
    };
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
        len: metadata.len(),
        modified: metadata.modified().ok(),
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
            |input| producer.run(input),
        ),
    };
    let visible_text = match (request.profile.visible_text, producers.visible_text) {
        (false, _) => Stage::NotRequested,
        (true, None) => stage_without_producer(cancellation),
        (true, Some(producer)) => run_stage(
            &context,
            cache,
            Modality::VisibleText,
            producer.identity(),
            |run: &VisibleTextRun| &run.receipt,
            CachedStage::VisibleText,
            |cached| match cached {
                CachedStage::VisibleText(run) => Some(run),
                _ => None,
            },
            |input| producer.run(input),
        ),
    };
    let scenes = match &request.profile.scenes {
        None => Stage::NotRequested,
        Some(config) => run_stage(
            &context,
            cache,
            Modality::Scene,
            config.producer_identity(),
            |run: &SceneRun| &run.receipt,
            CachedStage::Scenes,
            |cached| match cached {
                CachedStage::Scenes(run) => Some(run),
                _ => None,
            },
            |input| detect_scenes(input, config),
        ),
    };

    let analysis = compose(&CompositionInput {
        media: request.media.clone(),
        speech: speech.input(),
        visible_text: visible_text.input(),
        scenes: scenes.input(),
    })
    .map_err(AnalysisError::Composition)?;
    let count = |modality: Modality| {
        analysis
            .evidence
            .iter()
            .filter(|item| item.modality == modality)
            .count()
    };

    Ok(AnalysisReport {
        speech: speech.status(|run| &run.receipt, count(Modality::Speech)),
        visible_text: visible_text.status(|run| &run.receipt, count(Modality::VisibleText)),
        scenes: scenes.status(|run| &run.receipt, count(Modality::Scene)),
        analysis,
    })
}

fn stage_without_producer<R>(cancellation: &CancellationToken) -> Stage<R> {
    if cancellation.is_cancelled() {
        Stage::Cancelled
    } else {
        Stage::Unavailable
    }
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
    if let Some(run) = cache.get(&key).and_then(&unwrap) {
        return Stage::Completed { run, reused: true };
    }

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
    if let Some(failed) = changed("before") {
        return failed;
    }

    let input = ProducerInput {
        media: context.media,
        media_path: context.verified.path,
        input_hash: context.input_hash,
        cancellation: context.cancellation,
    };
    let run = match execute(&input) {
        Ok(run) => run,
        Err(ProducerError::Cancelled) => return Stage::Cancelled,
        Err(ProducerError::Failed { reason }) => {
            return Stage::Failed {
                receipt: Some(attempted),
                reason,
            }
        }
    };
    if let Some(failed) = changed("after") {
        return failed;
    }

    let receipt = receipt_of(&run);
    if let Some(reason) = receipt_mismatch(receipt, modality, &identity, context.input_hash) {
        return Stage::Failed {
            receipt: Some(receipt.clone()),
            reason,
        };
    }
    // Reject a run the core cannot compose here, so one bad producer run fails only its
    // own stage instead of the whole operation.
    if let Err(error) = compose(&single_stage_input(
        context.media,
        modality,
        &wrap(run.clone()),
    )) {
        return Stage::Failed {
            receipt: Some(receipt.clone()),
            reason: format!("producer run is not composable: {error}"),
        };
    }

    cache.put(key, wrap(run.clone()));
    Stage::Completed { run, reused: false }
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
        (Modality::VisibleText, CachedStage::VisibleText(run)) => {
            input.visible_text = ModalityInput::Completed {
                runs: vec![run.clone()],
            }
        }
        (Modality::Scene, CachedStage::Scenes(run)) => {
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
) -> Result<SceneRun, ProducerError> {
    let failed = |stage: &str, error: &dyn std::fmt::Display| ProducerError::Failed {
        reason: format!("visual-analysis {stage} failed: {error}"),
    };
    let mut source =
        FfmpegVideoSource::open(input.media_path).map_err(|error| failed("video open", &error))?;
    let result = video_analysis_ingest::surface::detect_content_scenes(
        &mut source,
        config.threshold,
        config.min_scene_len,
    )
    .map_err(|error| failed("scene detection", &error))?;
    drop(source);

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
    let identity = config.producer_identity();
    Ok(SceneRun {
        receipt: ProducerReceipt {
            config: Some(json!({
                "adapter": "video-analysis-ingest.detect_content_scenes",
                "threshold": config.threshold,
                "minSceneLen": config.min_scene_len,
                "visualAnalysisRevision": VISUAL_ANALYSIS_REVISION,
            })),
            ..attempted_receipt(Modality::Scene, &identity, input.input_hash)
        },
        scenes,
    })
}
