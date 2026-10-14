//! Implementation-authored regression tests for the Codex review findings on PR #31
//! (not part of the independent acceptance contract).

use std::cell::Cell;
use std::fs;
use std::path::Path;

use media_intelligence_analysis::{
    analyze, input_hash, AnalysisProfile, AnalysisRequest, AssetId, CachedStage, CancellationToken,
    ContentHash, InMemoryStageCache, MediaIdentity, Modality, ProducerError, ProducerIdentity,
    ProducerInput, ProducerReceipt, Producers, SpeechProducer, SpeechRun, StageCache, StageKey,
    StageStatus, VisibleTextProducer, VisibleTextRun,
};
use media_intelligence_core::{OcrObservation, TranscriptSegment};

fn identity(name: &str) -> ProducerIdentity {
    ProducerIdentity {
        producer: name.to_string(),
        producer_version: "1".to_string(),
        model: "m".to_string(),
        model_version: "1".to_string(),
        config_hash: "sha256:c".to_string(),
    }
}

fn receipt(modality: Modality, id: &ProducerIdentity, input: &str) -> ProducerReceipt {
    ProducerReceipt {
        modality,
        producer: id.producer.clone(),
        producer_version: id.producer_version.clone(),
        model: id.model.clone(),
        model_version: id.model_version.clone(),
        config_hash: id.config_hash.clone(),
        input_hash: input.to_string(),
        run_id: None,
        config: None,
    }
}

#[derive(Clone, Copy)]
enum Speech {
    /// Overwrite the media with equal-length bytes, restore its mtime, then succeed.
    SameSizeRewriteKeepingMtime,
    /// Overwrite the media, then fail.
    RewriteThenFail,
    /// Return a run whose receipt names the wrong modality.
    WrongModality,
}

struct FakeSpeech {
    behaviour: Speech,
    calls: Cell<usize>,
}

impl SpeechProducer for FakeSpeech {
    fn identity(&self) -> ProducerIdentity {
        identity("speech")
    }

    fn run(&self, input: &ProducerInput<'_>) -> Result<SpeechRun, ProducerError> {
        self.calls.set(self.calls.get() + 1);
        let mut modality = Modality::Speech;
        match self.behaviour {
            Speech::SameSizeRewriteKeepingMtime => rewrite_same_size(input.media_path),
            Speech::RewriteThenFail => {
                fs::write(input.media_path, b"different bytes entirely").unwrap();
                return Err(ProducerError::Failed {
                    reason: "boom".to_string(),
                });
            }
            Speech::WrongModality => modality = Modality::VisibleText,
        }
        Ok(SpeechRun {
            receipt: receipt(modality, &self.identity(), input.input_hash),
            segments: vec![TranscriptSegment {
                index: 0,
                start_seconds: 0.0,
                end_seconds: 1.0,
                text: "hello".to_string(),
                language: None,
                speaker: None,
                confidence: None,
            }],
        })
    }
}

struct FakeOcr {
    calls: Cell<usize>,
}

impl VisibleTextProducer for FakeOcr {
    fn identity(&self) -> ProducerIdentity {
        identity("ocr")
    }

    fn run(&self, input: &ProducerInput<'_>) -> Result<VisibleTextRun, ProducerError> {
        self.calls.set(self.calls.get() + 1);
        Ok(ocr_run(input.input_hash))
    }
}

fn ocr_run(input: &str) -> VisibleTextRun {
    VisibleTextRun {
        receipt: receipt(Modality::VisibleText, &identity("ocr"), input),
        observations: vec![OcrObservation {
            id: "o1".to_string(),
            text: "slide".to_string(),
            timestamp_seconds: 0.5,
            frame_index: None,
            language: None,
            region: None,
            confidence: None,
        }],
        tracks: vec![],
    }
}

fn rewrite_same_size(path: &Path) {
    let modified = fs::metadata(path).unwrap().modified().unwrap();
    let mut bytes = fs::read(path).unwrap();
    bytes[0] ^= 0xff;
    fs::write(path, &bytes).unwrap();
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
}

struct Fixture {
    _dir: tempfile::TempDir,
    request: AnalysisRequest,
}

fn fixture(speech: bool, visible_text: bool) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("media.bin");
    let bytes = b"original media bytes".to_vec();
    fs::write(&path, &bytes).unwrap();
    Fixture {
        request: AnalysisRequest {
            media: MediaIdentity {
                asset_id: AssetId::new("asset").unwrap(),
                content_hash: ContentHash::from_bytes(&bytes),
                duration_seconds: None,
            },
            media_path: path,
            profile: AnalysisProfile {
                speech,
                visible_text,
                scenes: None,
            },
        },
        _dir: dir,
    }
}

#[test]
fn same_size_rewrite_with_restored_mtime_is_detected() {
    let fixture = fixture(true, false);
    let speech = FakeSpeech {
        behaviour: Speech::SameSizeRewriteKeepingMtime,
        calls: Cell::new(0),
    };
    let mut cache = InMemoryStageCache::new();
    let report = analyze(
        &fixture.request,
        &Producers {
            speech: Some(&speech),
            visible_text: None,
        },
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(
        matches!(report.speech, StageStatus::Failed { .. }),
        "{:?}",
        report.speech
    );
    assert!(cache.is_empty());
}

#[test]
fn cached_stage_is_not_reused_after_an_earlier_producer_changed_the_media() {
    let fixture = fixture(true, true);
    let ocr = FakeOcr {
        calls: Cell::new(0),
    };
    let mut cache = InMemoryStageCache::new();
    let first = analyze(
        &AnalysisRequest {
            profile: AnalysisProfile {
                speech: false,
                visible_text: true,
                scenes: None,
            },
            ..fixture.request.clone()
        },
        &Producers {
            speech: None,
            visible_text: Some(&ocr),
        },
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        first.visible_text,
        StageStatus::CompletedWithFindings { .. }
    ));

    let speech = FakeSpeech {
        behaviour: Speech::RewriteThenFail,
        calls: Cell::new(0),
    };
    let report = analyze(
        &fixture.request,
        &Producers {
            speech: Some(&speech),
            visible_text: Some(&ocr),
        },
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(report.speech, StageStatus::Failed { .. }));
    assert!(
        matches!(report.visible_text, StageStatus::Failed { .. }),
        "{:?}",
        report.visible_text
    );
    assert!(report.analysis.evidence.is_empty());
}

#[test]
fn cache_entry_for_other_input_bytes_is_a_miss() {
    let fixture = fixture(false, true);
    let ocr = FakeOcr {
        calls: Cell::new(0),
    };
    let mut cache = InMemoryStageCache::new();
    cache.put(
        StageKey {
            modality: Modality::VisibleText,
            content_hash: fixture.request.media.content_hash.clone(),
            producer: ocr.identity(),
        },
        CachedStage::VisibleText(ocr_run("sha256:someone-elses-bytes")),
    );
    let report = analyze(
        &fixture.request,
        &Producers {
            speech: None,
            visible_text: Some(&ocr),
        },
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(ocr.calls.get(), 1, "invalid cache entry must not be reused");
    match &report.visible_text {
        StageStatus::CompletedWithFindings { receipt, .. } => {
            assert_eq!(
                receipt.input_hash,
                input_hash(&fixture.request.media.content_hash)
            )
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn wrong_modality_receipt_fails_only_its_stage() {
    let fixture = fixture(true, true);
    let speech = FakeSpeech {
        behaviour: Speech::WrongModality,
        calls: Cell::new(0),
    };
    let ocr = FakeOcr {
        calls: Cell::new(0),
    };
    let report = analyze(
        &fixture.request,
        &Producers {
            speech: Some(&speech),
            visible_text: Some(&ocr),
        },
        &mut InMemoryStageCache::new(),
        &CancellationToken::new(),
    )
    .expect("one bad producer run must not fail the whole call");
    match &report.speech {
        StageStatus::Failed { receipt, .. } => {
            assert_eq!(receipt.as_ref().map(|r| r.modality), Some(Modality::Speech))
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        report.visible_text,
        StageStatus::CompletedWithFindings { .. }
    ));
}

#[test]
fn scene_bound_identity_changes_when_only_the_intervals_change() {
    use media_intelligence_analysis::{scene_bound_identity, SceneRun};
    use media_intelligence_core::SceneInterval;

    let interval = |end: f64| SceneInterval {
        id: "scene-0".to_string(),
        index: 0,
        start_seconds: 0.0,
        end_seconds: end,
        start_frame: Some(0),
        end_frame: Some((end * 25.0) as u64),
    };
    let run = |end: f64| SceneRun {
        receipt: receipt(Modality::Scene, &identity("scenes"), "sha256:x"),
        scenes: vec![interval(end)],
    };
    let own = identity("ocr");
    assert_eq!(
        scene_bound_identity(&own, &run(5.0)),
        scene_bound_identity(&own, &run(5.0))
    );
    assert_ne!(
        scene_bound_identity(&own, &run(5.0)).config_hash,
        scene_bound_identity(&own, &run(6.0)).config_hash
    );
}

#[test]
fn scene_dependent_run_with_observations_outside_the_scenes_is_rejected() {
    use media_intelligence_analysis::{scene_bound_identity, SceneConfig, SceneRun};
    use std::cell::RefCell;

    struct OutsideOcr {
        received: RefCell<Option<SceneRun>>,
    }
    impl VisibleTextProducer for OutsideOcr {
        fn identity(&self) -> ProducerIdentity {
            identity("outside-ocr")
        }
        fn requires_scenes(&self) -> bool {
            true
        }
        fn run(&self, input: &ProducerInput<'_>) -> Result<VisibleTextRun, ProducerError> {
            let scenes = input.scenes.expect("scene run handed over");
            *self.received.borrow_mut() = Some(scenes.clone());
            let bound = scene_bound_identity(&self.identity(), scenes);
            let mut run = ocr_run(input.input_hash);
            run.receipt = receipt(Modality::VisibleText, &bound, input.input_hash);
            // Far beyond the end of any scene of the clip.
            run.observations[0].timestamp_seconds = 10_000.0;
            Ok(run)
        }
    }

    let Some(clip) = ffmpeg_clip() else { return };
    let ocr = OutsideOcr {
        received: RefCell::new(None),
    };
    let report = analyze(
        &AnalysisRequest {
            media: MediaIdentity {
                asset_id: AssetId::new("asset").unwrap(),
                content_hash: ContentHash::from_bytes(fs::read(&clip.1).unwrap()),
                duration_seconds: None,
            },
            media_path: clip.1.clone(),
            profile: AnalysisProfile {
                speech: false,
                visible_text: true,
                scenes: Some(SceneConfig::default()),
            },
        },
        &Producers {
            speech: None,
            visible_text: Some(&ocr),
        },
        &mut InMemoryStageCache::new(),
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(ocr.received.borrow().is_some());
    assert!(
        matches!(report.visible_text, StageStatus::Failed { .. }),
        "{:?}",
        report.visible_text
    );
}

/// A two-colour clip (scene detection needs a real video); `None` without ffmpeg.
fn ffmpeg_clip() -> Option<(tempfile::TempDir, std::path::PathBuf)> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("clip.mp4");
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=160x120:r=25:d=2",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=160x120:r=25:d=2",
            "-filter_complex",
            "[0:v][1:v]concat=n=2:v=1:a=0[v]",
            "-map",
            "[v]",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&path)
        .status()
        .ok()?;
    status.success().then_some((dir, path))
}
