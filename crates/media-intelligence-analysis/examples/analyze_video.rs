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

fn main() {
    todo!("implementation pending (#34)")
}
