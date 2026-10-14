//! #33: the visual-analysis visible-text adapter inside `analyze`, end to end.
//!
//! **No OCR model runs in these tests.** The real visual-analysis path runs: scene
//! detection, scene-aware frame sampling (`analyze_scene_aware_ocr`), the real
//! `JsonCommandOcrBackend` process round trip and `analyze_video_text_semantics` tracking.
//! The OCR command itself is an acceptance script (`tests/ocr_common`) that answers a fixed
//! slide text per scene colour. The ONNX backend is only exercised without model files.

#![cfg(feature = "visual-analysis-ocr")]

mod common;
mod ocr_common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use common::*;
use media_intelligence_analysis::visible_text::{
    OcrRequest, VisualAnalysisOcrConfig, VisualAnalysisOcrProducer, VISUAL_ANALYSIS_REVISION,
};
use media_intelligence_analysis::{
    analyze, input_hash, scene_bound_identity, AnalysisProfile, AssetId, CachedStage,
    CancellationToken, ContentHash, InMemoryStageCache, MediaIdentity, Modality, ProducerError,
    ProducerInput, Producers, SceneConfig, StageCache, StageKey, StageStatus, VisibleTextProducer,
};
use media_intelligence_core::{
    compose, CompositionInput, CoverageStatus, EvidenceContent, ModalityInput, RelationKind,
};
use ocr_common::*;

fn ocr_and_scenes() -> AnalysisProfile {
    AnalysisProfile {
        speech: false,
        visible_text: true,
        scenes: Some(SceneConfig::default()),
    }
}

fn adapter(config: VisualAnalysisOcrConfig) -> VisualAnalysisOcrProducer {
    VisualAnalysisOcrProducer::new(config).expect("local configuration")
}

fn producers(ocr: &VisualAnalysisOcrProducer) -> Producers<'_> {
    Producers {
        speech: None,
        visible_text: Some(ocr),
    }
}

fn failed_reason(status: &StageStatus) -> String {
    match status {
        StageStatus::Failed { reason, .. } => reason.clone(),
        other => panic!("expected Failed, got {other:?}"),
    }
}

/// `(start_frame, end_frame, start_seconds, end_seconds)` of each scene item, by index.
fn scene_ranges(
    analysis: &media_intelligence_analysis::EvidenceAnalysis,
) -> Vec<(u64, u64, f64, f64)> {
    let mut ranges: Vec<(u64, (u64, u64, f64, f64))> = analysis
        .evidence
        .iter()
        .filter_map(|item| match &item.content {
            EvidenceContent::Scene(scene) => Some((
                scene.scene_index,
                (
                    scene.start_frame.expect("frames"),
                    scene.end_frame.expect("frames"),
                    item.start_seconds,
                    item.end_seconds,
                ),
            )),
            _ => None,
        })
        .collect();
    ranges.sort_by_key(|(index, _)| *index);
    ranges.into_iter().map(|(_, range)| range).collect()
}

#[test]
fn scripted_ocr_end_to_end_collapses_repeated_slide_frames_into_one_item_per_scene() {
    let Some(clip) = slides_clip("ocr_e2e_collapse") else {
        return;
    };
    let (script, log) = write_ocr_script(clip.dir.path());
    let ocr = adapter(scripted_config(&script, &log));
    let mut cache = InMemoryStageCache::new();
    let request = request(&clip, ocr_and_scenes());
    let report = analyze(
        &request,
        &producers(&ocr),
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();

    // Scenes: the real visual-analysis run, three slides.
    assert!(
        matches!(
            report.scenes,
            StageStatus::CompletedWithFindings {
                evidence_count: 3,
                ..
            }
        ),
        "{:?}",
        report.scenes
    );
    let scenes = scene_run_from_evidence(&report.analysis);
    let ranges = scene_ranges(&report.analysis);

    // Receipt: adapter identity bound to this scene run, and this clip's input hash.
    let bound = scene_bound_identity(&ocr.identity(), &scenes);
    assert_eq!(bound.producer, "visual-analysis:json-command");
    assert_eq!(bound.producer_version, VISUAL_ANALYSIS_REVISION);
    assert_eq!(bound.model, SCRIPTED_OCR_NAME);
    assert_eq!(bound.model_version, SCRIPTED_OCR_VERSION);
    let receipt = match &report.visible_text {
        StageStatus::CompletedWithFindings {
            receipt,
            evidence_count: 3,
        } => receipt.clone(),
        other => panic!("expected three visible-text items, got {other:?}"),
    };
    assert_eq!(receipt.modality, Modality::VisibleText);
    assert_eq!(receipt.producer, bound.producer);
    assert_eq!(receipt.producer_version, bound.producer_version);
    assert_eq!(receipt.model, bound.model);
    assert_eq!(receipt.model_version, bound.model_version);
    assert_eq!(receipt.config_hash, bound.config_hash);
    assert_eq!(receipt.input_hash, input_hash(&clip.content_hash));
    assert_eq!(receipt.input_hash, expected_input_hash(&clip.content_hash));

    // Every sampled frame went through the command once and produced one observation.
    let calls = script_calls(&log);
    assert!(
        calls >= 9,
        "at least start, middle and last frame per scene: {calls}"
    );

    // One item per slide: a producer track holding every observation of that scene.
    let mut items = BTreeMap::new();
    let mut observation_total = 0;
    for item in &report.analysis.evidence {
        let EvidenceContent::VisibleText(text) = &item.content else {
            continue;
        };
        assert!(
            text.track_id.is_some(),
            "repeated frames come from a producer track"
        );
        assert_eq!(item.asset_id, request.media.asset_id);
        assert_eq!(item.content_hash, clip.content_hash);
        assert_eq!(item.receipt.input_hash, receipt.input_hash);
        observation_total += text.observations.len();
        items.insert(text.text.clone(), (item.clone(), text.clone()));
    }
    assert_eq!(
        items.keys().cloned().collect::<BTreeSet<_>>(),
        SLIDES.iter().map(|(_, text, _)| text.to_string()).collect(),
    );
    assert_eq!(
        observation_total, calls,
        "every observation belongs to exactly one slide item"
    );
    for (index, (_, slide, _)) in SLIDES.iter().enumerate() {
        let (item, text) = &items[*slide];
        let (start_frame, end_frame, start_seconds, end_seconds) = ranges[index];
        assert!(
            text.observations.len() >= 3,
            "{slide}: start, middle and last frame of its scene"
        );
        let ids: BTreeSet<_> = text
            .observations
            .iter()
            .map(|o| o.observation_id.clone())
            .collect();
        assert_eq!(
            ids.len(),
            text.observations.len(),
            "distinct observation ids"
        );
        for observation in &text.observations {
            let frame = observation.frame_index.expect("sampled frame index");
            assert!(
                (start_frame..end_frame).contains(&frame),
                "{slide}: frame {frame} outside its scene"
            );
            assert!((observation.timestamp_seconds - frame as f64 / 25.0).abs() < 1e-6);
            // The command reports confidence 90 (of 100); upstream scores it 0.9 (f32).
            let confidence = observation.ocr_confidence.expect("confidence");
            assert!((confidence - 0.9).abs() < 1e-6, "{confidence}");
        }
        // On the shared timeline, inside its scene.
        assert!(item.start_seconds >= start_seconds - 1e-9, "{slide}");
        assert!(item.end_seconds <= end_seconds + 1e-9, "{slide}");
    }

    // Scene -> visible-text relations link each slide to its own scene only.
    let label: BTreeMap<String, String> = report
        .analysis
        .evidence
        .iter()
        .map(|item| {
            let label = match &item.content {
                EvidenceContent::Scene(scene) => format!("scene#{}", scene.scene_index),
                EvidenceContent::VisibleText(text) => text.text.clone(),
                EvidenceContent::Speech(speech) => format!("speech#{}", speech.segment_index),
            };
            (item.id.clone(), label)
        })
        .collect();
    let scene_text: BTreeSet<(String, String)> = report
        .analysis
        .relations
        .iter()
        .filter(|relation| relation.kind == RelationKind::SceneVisibleText)
        .map(|relation| {
            (
                label[&relation.subject].clone(),
                label[&relation.object].clone(),
            )
        })
        .collect();
    assert_eq!(
        scene_text,
        BTreeSet::from([
            ("scene#0".to_string(), "RED SLIDE".to_string()),
            ("scene#1".to_string(), "GREEN SLIDE".to_string()),
            ("scene#2".to_string(), "BLUE SLIDE".to_string()),
        ])
    );

    // The run is cached under the bound identity, and the analysis is exactly the core's
    // composition of the scene run and that visible-text run.
    let key = StageKey {
        modality: Modality::VisibleText,
        content_hash: clip.content_hash.clone(),
        producer: bound,
    };
    let Some(CachedStage::VisibleText(run)) = cache.get(&key) else {
        panic!("visible-text run cached under the scene-bound identity");
    };
    assert_eq!(run.observations.len(), calls);
    assert_eq!(run.tracks.len(), 3);
    let recomposed = compose(&CompositionInput {
        media: request.media.clone(),
        speech: ModalityInput::NotRequested,
        visible_text: ModalityInput::Completed { runs: vec![run] },
        scenes: ModalityInput::Completed { runs: vec![scenes] },
    })
    .unwrap();
    assert_eq!(report.analysis, recomposed);
}

#[test]
fn unchanged_rerun_reuses_ocr_and_an_ocr_config_change_re_runs_only_ocr() {
    let Some(clip) = slides_clip("ocr_e2e_cache") else {
        return;
    };
    let (script, log) = write_ocr_script(clip.dir.path());
    let config = scripted_config(&script, &log);
    let ocr = adapter(config.clone());
    let mut cache = InMemoryStageCache::new();
    let first = analyze(
        &request(&clip, ocr_and_scenes()),
        &producers(&ocr),
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    let calls = script_calls(&log);
    assert!(calls > 0);

    let rerun = analyze(
        &request(&clip, ocr_and_scenes()),
        &producers(&ocr),
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(rerun.scenes, StageStatus::Reused { .. }));
    assert!(matches!(
        rerun.visible_text,
        StageStatus::Reused {
            evidence_count: 3,
            ..
        }
    ));
    assert_eq!(script_calls(&log), calls, "OCR did not run again");
    assert_eq!(rerun.analysis, first.analysis);

    // Changing the OCR request changes the identity: OCR runs again, scenes are reused.
    let changed = adapter(VisualAnalysisOcrConfig {
        request: OcrRequest::default().languages(["en"]),
        ..config
    });
    assert_ne!(changed.identity().config_hash, ocr.identity().config_hash);
    let third = analyze(
        &request(&clip, ocr_and_scenes()),
        &producers(&changed),
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(third.scenes, StageStatus::Reused { .. }));
    assert!(matches!(
        third.visible_text,
        StageStatus::CompletedWithFindings {
            evidence_count: 3,
            ..
        }
    ));
    assert_eq!(script_calls(&log), 2 * calls, "only OCR re-executed");
    assert_eq!(
        evidence_ids(&third.analysis, Modality::Scene),
        evidence_ids(&first.analysis, Modality::Scene)
    );
}

#[test]
fn missing_onnx_model_files_fail_ocr_only() {
    let Some(clip) = slides_clip("ocr_e2e_missing_model") else {
        return;
    };
    let missing = clip.dir.path().join("no-model-runtime-bundles");
    let ocr = adapter(onnx_config(&missing));
    let mut cache = InMemoryStageCache::new();
    let report = analyze(
        &request(&clip, ocr_and_scenes()),
        &producers(&ocr),
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        report.scenes,
        StageStatus::CompletedWithFindings {
            evidence_count: 3,
            ..
        }
    ));
    let reason = failed_reason(&report.visible_text);
    assert!(reason.contains("bundle_root"), "{reason}");
    assert!(
        reason.contains(&missing.display().to_string()),
        "the reason names the path: {reason}"
    );
    assert!(matches!(
        coverage(&report.analysis, Modality::VisibleText),
        CoverageStatus::Failed { .. }
    ));
    assert_eq!(count(&report.analysis, Modality::VisibleText), 0);
    assert_eq!(cache.len(), 1, "only the scene run is cached");

    // A rerun retries OCR (failures are not cached) and reuses scenes.
    let rerun = analyze(
        &request(&clip, ocr_and_scenes()),
        &producers(&ocr),
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(rerun.scenes, StageStatus::Reused { .. }));
    assert!(matches!(rerun.visible_text, StageStatus::Failed { .. }));
}

#[test]
fn an_empty_onnx_bundle_root_fails_ocr_only() {
    let Some(clip) = slides_clip("ocr_e2e_empty_bundle") else {
        return;
    };
    let empty = clip.dir.path().join("empty-model-runtime");
    std::fs::create_dir(&empty).unwrap();
    let ocr = adapter(onnx_config(&empty));
    let report = analyze(
        &request(&clip, ocr_and_scenes()),
        &producers(&ocr),
        &mut InMemoryStageCache::new(),
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        report.scenes,
        StageStatus::CompletedWithFindings { .. }
    ));
    let reason = failed_reason(&report.visible_text);
    assert!(!reason.trim().is_empty());
}

#[test]
fn scenes_not_requested_fail_the_adapter_with_an_actionable_reason() {
    let Some(clip) = slides_clip("ocr_e2e_no_scenes") else {
        return;
    };
    let (script, log) = write_ocr_script(clip.dir.path());
    let ocr = adapter(scripted_config(&script, &log));
    let mut cache = InMemoryStageCache::new();
    let report = analyze(
        &request(
            &clip,
            AnalysisProfile {
                speech: false,
                visible_text: true,
                scenes: None,
            },
        ),
        &producers(&ocr),
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    let reason = failed_reason(&report.visible_text);
    assert!(reason.to_lowercase().contains("scene"), "{reason}");
    assert_eq!(report.scenes, StageStatus::NotRequested);
    assert_eq!(script_calls(&log), 0, "no OCR without the scene run");
    assert!(cache.is_empty(), "no hidden scene detection");
}

#[test]
fn direct_run_without_a_scene_run_fails_before_any_ocr() {
    let Some(clip) = slides_clip("ocr_direct_no_scenes") else {
        return;
    };
    let (script, log) = write_ocr_script(clip.dir.path());
    let ocr = adapter(scripted_config(&script, &log));
    let media = media_for(&clip.path, clip.content_hash.clone());
    let hash = input_hash(&media.content_hash);
    let outcome = ocr.run(&ProducerInput {
        media: &media,
        media_path: &clip.path,
        input_hash: &hash,
        cancellation: &CancellationToken::new(),
        scenes: None,
    });
    match outcome {
        Err(ProducerError::Failed { reason }) => {
            assert!(reason.to_lowercase().contains("scene"), "{reason}")
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(script_calls(&log), 0);
}

#[test]
fn cancelled_before_the_run_returns_cancelled_without_touching_anything() {
    // Missing media, missing model bundle, no scene run: only the token may decide.
    let ocr = adapter(onnx_config(&PathBuf::from("/does/not/exist/bundles")));
    let media = MediaIdentity {
        asset_id: AssetId::new("acceptance:issue-33-cancelled").unwrap(),
        content_hash: ContentHash::from_bytes(b"not read"),
        duration_seconds: None,
    };
    let media_path = PathBuf::from("/does/not/exist/clip.mp4");
    let hash = input_hash(&media.content_hash);
    let token = CancellationToken::new();
    token.cancel();
    let outcome = ocr.run(&ProducerInput {
        media: &media,
        media_path: &media_path,
        input_hash: &hash,
        cancellation: &token,
        scenes: None,
    });
    assert_eq!(outcome, Err(ProducerError::Cancelled));
}

#[test]
fn cancelled_analysis_reports_ocr_cancelled_and_runs_no_command() {
    let Some(clip) = slides_clip("ocr_e2e_cancelled") else {
        return;
    };
    let (script, log) = write_ocr_script(clip.dir.path());
    let ocr = adapter(scripted_config(&script, &log));
    let token = CancellationToken::new();
    token.cancel();
    let mut cache = InMemoryStageCache::new();
    let report = analyze(
        &request(&clip, ocr_and_scenes()),
        &producers(&ocr),
        &mut cache,
        &token,
    )
    .unwrap();
    assert_eq!(report.visible_text, StageStatus::Cancelled);
    assert_eq!(report.scenes, StageStatus::Cancelled);
    assert_eq!(script_calls(&log), 0);
    assert!(cache.is_empty());
}
