//! #33: download-capable OCR backend configurations are refused at construction unless the
//! caller explicitly allows network access (mirrors #32's `ModelFetchPolicy`). Nothing runs.

#![cfg(feature = "visual-analysis-ocr")]

use std::path::PathBuf;

use media_intelligence_analysis::visible_text::{
    OcrAdapterError, OcrBackendConfig, OcrLocalModelOptions, VisualAnalysisOcrConfig,
    VisualAnalysisOcrProducer,
};
use media_intelligence_analysis::ModelFetchPolicy;

fn onnx(options: OcrLocalModelOptions, policy: ModelFetchPolicy) -> VisualAnalysisOcrConfig {
    let mut config = VisualAnalysisOcrConfig::new(OcrBackendConfig::OnnxTrOcr(options));
    config.model_fetch = policy;
    config
}

fn downloading() -> OcrLocalModelOptions {
    OcrLocalModelOptions {
        bundle_root: PathBuf::from("/does/not/exist/model-runtime"),
        auto_download: true,
        ..OcrLocalModelOptions::default()
    }
}

#[test]
fn onnx_auto_download_is_refused_without_permission() {
    match VisualAnalysisOcrProducer::new(onnx(downloading(), ModelFetchPolicy::LocalOnly)) {
        Err(OcrAdapterError::ModelFetchNotAllowed { reason }) => {
            assert!(
                reason.contains("auto_download"),
                "reason names the download-capable option: {reason}"
            );
            assert!(
                reason.contains("bundle_root"),
                "reason names the local alternative: {reason}"
            );
        }
        Err(other) => panic!("unexpected refusal: {other:?}"),
        Ok(_) => panic!("download-capable configuration accepted without permission"),
    }
}

#[test]
fn onnx_auto_download_is_accepted_with_explicit_permission() {
    let producer =
        VisualAnalysisOcrProducer::new(onnx(downloading(), ModelFetchPolicy::AllowNetwork))
            .expect("the caller allowed network access");
    assert_eq!(
        producer.config().model_fetch,
        ModelFetchPolicy::AllowNetwork
    );
}

#[test]
fn local_configurations_are_accepted_without_touching_the_file_system() {
    // Neither path exists; construction must not check them (a missing bundle is a
    // run-time failure, because files may appear between construction and run).
    let local_onnx = OcrLocalModelOptions {
        bundle_root: PathBuf::from("/does/not/exist/model-runtime"),
        hf_token: Some("hf_token_without_auto_download".to_string()),
        ..OcrLocalModelOptions::default()
    };
    VisualAnalysisOcrProducer::new(onnx(local_onnx, ModelFetchPolicy::LocalOnly))
        .expect("ONNX without auto_download never downloads");
    let command = VisualAnalysisOcrConfig::new(OcrBackendConfig::JsonCommand {
        name: "caller-ocr".to_string(),
        command: PathBuf::from("/does/not/exist/ocr-command"),
        args: vec![],
    });
    VisualAnalysisOcrProducer::new(command)
        .expect("the caller's own command is not a model download");
}
