//! #33: identity and configuration hash of the visual-analysis visible-text adapter.
//! Pure construction; nothing runs.

#![cfg(feature = "visual-analysis-ocr")]

use std::collections::BTreeSet;
use std::path::PathBuf;

use media_intelligence_analysis::visible_text::{
    ocr::OcrTechnique, OcrBackendConfig, OcrLocalModelOptions, OcrPreset, OcrRequest,
    VisualAnalysisOcrConfig, VisualAnalysisOcrProducer, JSON_COMMAND_KIND, OCR_PRODUCER_PREFIX,
    ONNX_TROCR_KIND, UNSPECIFIED_MODEL_VERSION, VISUAL_ANALYSIS_REVISION,
};
use media_intelligence_analysis::{ModelFetchPolicy, VisibleTextProducer};

fn json_backend() -> OcrBackendConfig {
    OcrBackendConfig::JsonCommand {
        name: "tesseract-5-eng".to_string(),
        command: PathBuf::from("/opt/ocr/run-tesseract"),
        args: vec!["--psm".to_string(), "6".to_string(), "{image}".to_string()],
    }
}

fn onnx_options() -> OcrLocalModelOptions {
    OcrLocalModelOptions {
        bundle_root: PathBuf::from("/models/model-runtime"),
        ..OcrLocalModelOptions::default()
    }
}

fn onnx_backend() -> OcrBackendConfig {
    OcrBackendConfig::OnnxTrOcr(onnx_options())
}

fn producer(config: VisualAnalysisOcrConfig) -> VisualAnalysisOcrProducer {
    VisualAnalysisOcrProducer::new(config).expect("local configuration is accepted")
}

fn assert_sha256(hash: &str) {
    let hex = hash.strip_prefix("sha256:").expect("sha256: prefix");
    assert_eq!(hex.len(), 64, "{hash}");
    assert!(
        hex.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "{hash}"
    );
}

#[test]
fn constants_name_the_pinned_revision_and_backend_kinds() {
    assert_eq!(
        VISUAL_ANALYSIS_REVISION,
        "115c1f0caceb644993df3121b4625e674a9f1b7a"
    );
    assert_eq!(OCR_PRODUCER_PREFIX, "visual-analysis");
    assert_eq!(ONNX_TROCR_KIND, "onnx-trocr");
    assert_eq!(JSON_COMMAND_KIND, "json-command");
    assert_eq!(UNSPECIFIED_MODEL_VERSION, "unspecified");
}

#[test]
fn new_uses_the_default_request_no_version_and_local_only() {
    let config = VisualAnalysisOcrConfig::new(json_backend());
    assert_eq!(config.backend, json_backend());
    assert_eq!(config.request, OcrRequest::default());
    assert_eq!(config.model_version, None);
    assert_eq!(config.model_fetch, ModelFetchPolicy::LocalOnly);
    assert_eq!(ModelFetchPolicy::default(), ModelFetchPolicy::LocalOnly);
}

#[test]
fn backend_kind_and_model_id() {
    assert_eq!(json_backend().kind(), JSON_COMMAND_KIND);
    assert_eq!(json_backend().model_id(), "tesseract-5-eng");
    assert_eq!(onnx_backend().kind(), ONNX_TROCR_KIND);
    assert_eq!(onnx_backend().model_id(), "Xenova/trocr-base-printed");
    let handwritten = OcrBackendConfig::OnnxTrOcr(OcrLocalModelOptions {
        preset: OcrPreset::TrOcrBaseHandwrittenOnnx,
        ..onnx_options()
    });
    assert_eq!(handwritten.model_id(), "Xenova/trocr-base-handwritten");
}

#[test]
fn identity_names_backend_revision_model_version_and_config_hash() {
    for backend in [json_backend(), onnx_backend()] {
        let mut config = VisualAnalysisOcrConfig::new(backend.clone());
        let unversioned = producer(config.clone()).identity();
        assert_eq!(
            unversioned.producer,
            format!("visual-analysis:{}", backend.kind())
        );
        assert_eq!(unversioned.producer_version, VISUAL_ANALYSIS_REVISION);
        assert_eq!(unversioned.model, backend.model_id());
        assert_eq!(unversioned.model_version, UNSPECIFIED_MODEL_VERSION);
        assert_eq!(unversioned.config_hash, config.config_hash());

        config.model_version = Some("bundle-2026-10".to_string());
        let versioned = producer(config.clone()).identity();
        assert_eq!(versioned.model_version, "bundle-2026-10");
        assert_eq!(
            versioned.config_hash, unversioned.config_hash,
            "the declared version is its own identity field, not part of the hash"
        );
        assert_ne!(versioned, unversioned);
    }
    assert_eq!(
        producer(VisualAnalysisOcrConfig::new(json_backend()))
            .identity()
            .producer,
        "visual-analysis:json-command"
    );
    assert_eq!(
        producer(VisualAnalysisOcrConfig::new(onnx_backend()))
            .identity()
            .producer,
        "visual-analysis:onnx-trocr"
    );
}

#[test]
fn the_adapter_requires_the_scene_run() {
    assert!(producer(VisualAnalysisOcrConfig::new(json_backend())).requires_scenes());
    assert!(producer(VisualAnalysisOcrConfig::new(onnx_backend())).requires_scenes());
}

#[test]
fn config_hash_is_a_deterministic_sha256() {
    let a = VisualAnalysisOcrConfig::new(json_backend());
    let b = VisualAnalysisOcrConfig::new(json_backend());
    assert_sha256(&a.config_hash());
    assert_eq!(a.config_hash(), b.config_hash());
    assert_sha256(&VisualAnalysisOcrConfig::new(onnx_backend()).config_hash());
}

#[test]
fn every_result_relevant_option_changes_the_hash() {
    let json = VisualAnalysisOcrConfig::new(json_backend());
    let onnx = VisualAnalysisOcrConfig::new(onnx_backend());
    let with_json = |backend: OcrBackendConfig| VisualAnalysisOcrConfig {
        backend,
        ..json.clone()
    };
    let with_request = |request: OcrRequest| VisualAnalysisOcrConfig {
        request,
        ..json.clone()
    };
    let with_onnx = |options: OcrLocalModelOptions| VisualAnalysisOcrConfig {
        backend: OcrBackendConfig::OnnxTrOcr(options),
        ..onnx.clone()
    };
    let variants: Vec<(&str, VisualAnalysisOcrConfig)> = vec![
        (
            "json name",
            with_json(OcrBackendConfig::JsonCommand {
                name: "tesseract-5-deu".to_string(),
                command: PathBuf::from("/opt/ocr/run-tesseract"),
                args: vec!["--psm".to_string(), "6".to_string(), "{image}".to_string()],
            }),
        ),
        (
            "json command",
            with_json(OcrBackendConfig::JsonCommand {
                name: "tesseract-5-eng".to_string(),
                command: PathBuf::from("/opt/ocr/other-command"),
                args: vec!["--psm".to_string(), "6".to_string(), "{image}".to_string()],
            }),
        ),
        (
            "json args",
            with_json(OcrBackendConfig::JsonCommand {
                name: "tesseract-5-eng".to_string(),
                command: PathBuf::from("/opt/ocr/run-tesseract"),
                args: vec!["--psm".to_string(), "11".to_string(), "{image}".to_string()],
            }),
        ),
        (
            "request languages",
            with_request(OcrRequest::default().languages(["de"])),
        ),
        (
            "request preserve_layout",
            with_request(OcrRequest::default().preserve_layout(false)),
        ),
        (
            "request include_tokens",
            with_request(OcrRequest::default().include_tokens(false)),
        ),
        (
            "request min_confidence",
            with_request(OcrRequest::default().min_confidence(40)),
        ),
        (
            "request attributes",
            with_request(OcrRequest::default().attribute("psm", "6")),
        ),
        (
            "request technique",
            with_request(OcrRequest::default().technique(OcrTechnique::Heuristic)),
        ),
        ("backend kind", onnx.clone()),
        (
            "onnx preset",
            with_onnx(OcrLocalModelOptions {
                preset: OcrPreset::TrOcrBaseHandwrittenOnnx,
                ..onnx_options()
            }),
        ),
        (
            "onnx bundle_root",
            with_onnx(OcrLocalModelOptions {
                bundle_root: PathBuf::from("/other/models"),
                ..onnx_options()
            }),
        ),
        (
            "onnx cache_dir",
            with_onnx(OcrLocalModelOptions {
                cache_dir: Some(PathBuf::from("/hf-cache")),
                ..onnx_options()
            }),
        ),
    ];
    let base = json.config_hash();
    let mut hashes = BTreeSet::from([base.clone()]);
    for (name, config) in &variants {
        let hash = config.config_hash();
        assert_sha256(&hash);
        if *name != "onnx preset" && *name != "onnx bundle_root" && *name != "onnx cache_dir" {
            assert_ne!(hash, base, "{name} must change the hash");
        } else {
            assert_ne!(hash, onnx.config_hash(), "{name} must change the hash");
        }
        hashes.insert(hash);
    }
    assert_eq!(hashes.len(), variants.len() + 1, "all hashes differ");
}

#[test]
fn permission_version_and_download_settings_do_not_change_the_hash() {
    let onnx = VisualAnalysisOcrConfig::new(onnx_backend());
    let base = onnx.config_hash();
    let download_settings = VisualAnalysisOcrConfig {
        backend: OcrBackendConfig::OnnxTrOcr(OcrLocalModelOptions {
            auto_download: true,
            download_progress: true,
            hf_token: Some("hf_secret_token".to_string()),
            max_retries: 7,
            ..onnx_options()
        }),
        model_fetch: ModelFetchPolicy::AllowNetwork,
        model_version: Some("v2".to_string()),
        ..onnx.clone()
    };
    assert_eq!(download_settings.config_hash(), base);
    let json = VisualAnalysisOcrConfig::new(json_backend());
    let allowed = VisualAnalysisOcrConfig {
        model_fetch: ModelFetchPolicy::AllowNetwork,
        ..json.clone()
    };
    assert_eq!(allowed.config_hash(), json.config_hash());
}
