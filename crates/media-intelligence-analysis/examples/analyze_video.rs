//! Native video-analysis example (#34): real speech (Candle Whisper via audio-analysis),
//! canonical scenes and real visible text (ONNX TrOCR via visual-analysis) on one local
//! media file, composed by `analyze` and printed as report JSON.
//!
//! Built only with `--features audio-analysis-candle,audio-analysis-media,visual-analysis-ocr-onnx`
//! (`required-features` in Cargo.toml).
//!
//! # CLI contract (fixed by the #34 acceptance; the body is pending)
//!
//! ```text
//! analyze_video <MEDIA_PATH> [OPTIONS]
//!
//!   <MEDIA_PATH>                 local media file; never downloaded. Its SHA-256 is computed
//!                                and declared as the content hash.
//!   --asset-id <ID>              caller asset id (default: `local:<file name>`)
//!   --whisper-model-dir <DIR>    Candle Whisper model location. Without --allow-network:
//!                                a local bundle directory (config.json,
//!                                generation_config.json, tokenizer.json,
//!                                preprocessor_config.json, model.safetensors) used as
//!                                `CandleWhisperOptions::model_bundle`. With --allow-network:
//!                                the cache directory audio-analysis downloads into once
//!                                (`model_dir`, needs audio-analysis `model-bundles`).
//!   --whisper-model <ID>         Whisper model id (default `openai/whisper-base.en`)
//!   --trocr-bundle-root <DIR>    `OcrLocalModelOptions::bundle_root` for
//!                                `Xenova/trocr-base-printed`
//!   --trocr-cache-dir <DIR>      optional `OcrLocalModelOptions::cache_dir`
//!   --allow-network              ModelFetchPolicy::AllowNetwork for both adapters: allows the
//!                                one-time model download (Whisper download path, TrOCR
//!                                `auto_download`). Without it nothing touches the network
//!                                and download-capable configurations are refused.
//!   --no-speech | --no-ocr | --no-scenes
//!                                leave that modality out of the AnalysisProfile (it is then
//!                                `not_requested` in the report)
//! ```
//!
//! Behaviour:
//!
//! * Prints exactly one `AnalysisReport` as pretty JSON on stdout (statuses for speech,
//!   visible text and scenes, plus the composed `EvidenceAnalysis`); diagnostics go to
//!   stderr.
//! * A requested modality whose model location flag is missing is reported as
//!   `unavailable` (no producer supplied), not as an error and never as completed. A model
//!   location that does not exist is that stage's `failed` status with the adapter's reason;
//!   other stages still run.
//! * `--no-scenes` with OCR requested yields OCR `failed` naming the scene stage (the OCR
//!   adapter samples scene intervals, #33).
//! * Exit status 0 when a report was produced (whatever the stage statuses); non-zero with
//!   a message on stderr for usage errors, a missing media file, a refused download-capable
//!   configuration (adapter construction error) or `AnalysisError`.
//! * Uses an in-process `InMemoryStageCache`; nothing is persisted.
//!
//! Example (models prepared once with `--allow-network`, then local only):
//!
//! ```sh
//! cargo run --release -p media-intelligence-analysis --example analyze_video \
//!   --features audio-analysis-candle,audio-analysis-media,visual-analysis-ocr-onnx -- \
//!   crates/media-intelligence-analysis/tests/fixtures/model_acceptance/wcc-excerpt.webm \
//!   --whisper-model-dir ~/.cache/media-intelligence-acceptance/models/whisper-base.en \
//!   --trocr-bundle-root ~/.cache/media-intelligence-acceptance/models/trocr
//! ```

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use media_intelligence_analysis::speech::{
    AudioAnalysisSpeechConfig, AudioAnalysisSpeechProducer, CandleWhisperOptions,
    TranscriptionProviderSelection,
};
use media_intelligence_analysis::visible_text::{
    OcrBackendConfig, OcrLocalModelOptions, OcrPreset, VisualAnalysisOcrConfig,
    VisualAnalysisOcrProducer,
};
use media_intelligence_analysis::{
    analyze, AnalysisProfile, AnalysisRequest, AssetId, CancellationToken, ContentHash,
    InMemoryStageCache, MediaIdentity, ModelFetchPolicy, Producers, SceneConfig,
};

struct Args {
    media: PathBuf,
    asset_id: Option<String>,
    whisper_dir: Option<PathBuf>,
    whisper_model: String,
    trocr_root: Option<PathBuf>,
    trocr_cache: Option<PathBuf>,
    allow_network: bool,
    speech: bool,
    ocr: bool,
    scenes: bool,
}

fn parse() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let mut parsed = Args {
        media: PathBuf::new(),
        asset_id: None,
        whisper_dir: None,
        whisper_model: "openai/whisper-base.en".to_string(),
        trocr_root: None,
        trocr_cache: None,
        allow_network: false,
        speech: true,
        ocr: true,
        scenes: true,
    };
    let mut media = None;
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--asset-id" => parsed.asset_id = Some(value("--asset-id")?),
            "--whisper-model-dir" => {
                parsed.whisper_dir = Some(value("--whisper-model-dir")?.into())
            }
            "--whisper-model" => parsed.whisper_model = value("--whisper-model")?,
            "--trocr-bundle-root" => parsed.trocr_root = Some(value("--trocr-bundle-root")?.into()),
            "--trocr-cache-dir" => parsed.trocr_cache = Some(value("--trocr-cache-dir")?.into()),
            "--allow-network" => parsed.allow_network = true,
            "--no-speech" => parsed.speech = false,
            "--no-ocr" => parsed.ocr = false,
            "--no-scenes" => parsed.scenes = false,
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            path if media.is_none() => media = Some(PathBuf::from(path)),
            extra => return Err(format!("unexpected argument {extra}")),
        }
    }
    parsed.media = media.ok_or("usage: analyze_video <MEDIA_PATH> [OPTIONS]")?;
    Ok(parsed)
}

fn content_hash(path: &PathBuf) -> std::io::Result<ContentHash> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1 << 16];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let hex: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(ContentHash::parse(hex).expect("sha256 hex"))
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("analyze_video: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args = parse()?;
    if !args.media.is_file() {
        return Err(format!(
            "media file `{}` does not exist",
            args.media.display()
        ));
    }
    let policy = if args.allow_network {
        ModelFetchPolicy::AllowNetwork
    } else {
        ModelFetchPolicy::LocalOnly
    };
    let asset_id = args.asset_id.clone().unwrap_or_else(|| {
        format!(
            "local:{}",
            args.media.file_name().unwrap_or_default().to_string_lossy()
        )
    });
    let media = MediaIdentity {
        asset_id: AssetId::new(asset_id).map_err(|e| e.to_string())?,
        content_hash: content_hash(&args.media).map_err(|e| e.to_string())?,
        duration_seconds: None,
    };

    let speech = match &args.whisper_dir {
        Some(dir) if args.speech => {
            let options = if args.allow_network {
                CandleWhisperOptions {
                    model_id: args.whisper_model.clone(),
                    model_dir: Some(dir.clone()),
                    ..CandleWhisperOptions::default()
                }
            } else {
                CandleWhisperOptions {
                    model_id: args.whisper_model.clone(),
                    model_bundle: Some(dir.clone()),
                    ..CandleWhisperOptions::default()
                }
            };
            let mut config = AudioAnalysisSpeechConfig::new(
                TranscriptionProviderSelection::CandleWhisper(options),
            );
            config.model_fetch = policy;
            Some(AudioAnalysisSpeechProducer::new(config).map_err(|e| e.to_string())?)
        }
        _ => None,
    };
    let ocr = match &args.trocr_root {
        Some(root) if args.ocr => {
            let mut config =
                VisualAnalysisOcrConfig::new(OcrBackendConfig::OnnxTrOcr(OcrLocalModelOptions {
                    preset: OcrPreset::TrOcrBasePrintedOnnx,
                    bundle_root: root.clone(),
                    auto_download: args.allow_network,
                    cache_dir: args.trocr_cache.clone(),
                    ..OcrLocalModelOptions::default()
                }));
            config.model_fetch = policy;
            Some(VisualAnalysisOcrProducer::new(config).map_err(|e| e.to_string())?)
        }
        _ => None,
    };
    for (requested, present, name) in [
        (
            args.speech,
            speech.is_some(),
            "speech (--whisper-model-dir)",
        ),
        (
            args.ocr,
            ocr.is_some(),
            "visible text (--trocr-bundle-root)",
        ),
    ] {
        if requested && !present {
            eprintln!("note: no model location for {name}; that stage is unavailable");
        }
    }

    let request = AnalysisRequest {
        media,
        media_path: args.media.clone(),
        profile: AnalysisProfile {
            speech: args.speech,
            visible_text: args.ocr,
            scenes: args.scenes.then(SceneConfig::default),
        },
    };
    let producers = Producers {
        speech: speech.as_ref().map(|p| p as _),
        visible_text: ocr.as_ref().map(|p| p as _),
    };
    let report = analyze(
        &request,
        &producers,
        &mut InMemoryStageCache::new(),
        &CancellationToken::new(),
    )
    .map_err(|e| e.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
    );
    Ok(())
}
