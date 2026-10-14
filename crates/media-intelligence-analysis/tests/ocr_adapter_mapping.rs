//! #33: pure mapping of upstream scene-aware OCR output into a `VisibleTextRun`.
//!
//! The input is produced by the real upstream `analyze_scene_aware_ocr` at the pinned
//! revision, over an in-memory fixture frame source and a fixture `OcrBackend` written by
//! the acceptance agent (fixed text per frame; no OCR model, no ffmpeg). The fixture is
//! visual-analysis' own `scene_aware_ocr_runs_only_on_representative_frames_and_builds_tracks`
//! scenario: two scenes [0,5) and [5,9) at 30 fps, sampled frames 0,2,4 | 5,6,8, and two
//! upstream tracks of three samples each.

#![cfg(feature = "visual-analysis-ocr")]

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use image_analysis_core::ImageView;
use media_intelligence_analysis::visible_text::{
    ocr::{OcrBackend, OcrDocument, OcrTextBlock},
    recognition::VideoTextSemanticAnalysis,
    video_core::{
        BoundingBox, FramePosition, OwnedVideoFrame, PixelFormat, Result as UpstreamResult, Scene,
    },
    visible_text_run_from_analysis, OcrRequest, SceneAwareOcrAnalysis,
};
use media_intelligence_analysis::{Modality, ProducerError, ProducerIdentity};
use media_intelligence_core::{
    compose, AssetId, CompositionInput, ContentHash, EvidenceContent, MediaIdentity, ModalityInput,
    Region,
};
use num_rational::Rational64;
use video_analysis_ingest::surface::scene_ocr::analyze_scene_aware_ocr;
use video_analysis_ingest::{MediaSourceInfo, VideoFrameSource, VideoStreamInfo};

const INPUT_HASH: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";

struct FixtureSource {
    info: MediaSourceInfo,
    frames: VecDeque<OwnedVideoFrame>,
}

impl VideoFrameSource for FixtureSource {
    fn source_info(&self) -> &MediaSourceInfo {
        &self.info
    }

    fn next_video_frame(&mut self) -> UpstreamResult<Option<OwnedVideoFrame>> {
        Ok(self.frames.pop_front())
    }
}

/// Frames 0..5 carry pixel value 10 ("First Slide": document text, confidence 75,
/// language en); frames 5..9 carry 200 ("Second Slide": one block with a region and
/// confidence 50).
struct FixtureOcr;

impl OcrBackend for FixtureOcr {
    fn recognize_image(
        &mut self,
        image: &ImageView<'_>,
        _request: &OcrRequest,
    ) -> UpstreamResult<OcrDocument> {
        if image.data[0] < 100 {
            Ok(OcrDocument::new("First Slide", image.width, image.height)?
                .language("en")
                .confidence(75))
        } else {
            Ok(
                OcrDocument::new("Second Slide", image.width, image.height)?.block(
                    OcrTextBlock::paragraph("Second Slide")?
                        .region(BoundingBox {
                            x: 0,
                            y: 0,
                            width: 2,
                            height: 1,
                        })
                        .confidence(50),
                ),
            )
        }
    }
}

fn position(frame_index: u64) -> FramePosition {
    FramePosition::from_frame_index(frame_index, Rational64::new(30, 1))
}

fn recorded_output() -> SceneAwareOcrAnalysis {
    let info = MediaSourceInfo::recorded("fixture").with_video(VideoStreamInfo {
        width: 2,
        height: 2,
        frame_rate: Some(Rational64::new(30, 1)),
        pixel_format: PixelFormat::Rgb24,
    });
    let frames = (0..=8)
        .map(|index| OwnedVideoFrame {
            position: position(index),
            width: 2,
            height: 2,
            pixel_format: PixelFormat::Rgb24,
            data: vec![if index < 5 { 10 } else { 200 }; 12],
            stride: 6,
        })
        .collect();
    let scenes = [
        Scene {
            start: position(0),
            end: position(5),
        },
        Scene {
            start: position(5),
            end: position(9),
        },
    ];
    let output = analyze_scene_aware_ocr(
        &mut FixtureSource { info, frames },
        FixtureOcr,
        OcrRequest::default(),
        "fixture-video",
        &scenes,
        Some(8.0 / 30.0),
    )
    .expect("upstream scene-aware OCR over fixture frames");
    // Preconditions: the upstream behaviour this test relies on.
    assert_eq!(output.sampled_frames, vec![0, 2, 4, 5, 6, 8]);
    assert_eq!(output.observations.len(), 6);
    assert_eq!(output.text_semantics.tracks.len(), 2);
    assert!(output
        .text_semantics
        .tracks
        .iter()
        .all(|track| track.sample_count == 3));
    output
}

fn identity() -> ProducerIdentity {
    ProducerIdentity {
        producer: "visual-analysis:json-command".to_string(),
        producer_version: "115c1f0caceb644993df3121b4625e674a9f1b7a".to_string(),
        model: "fixture-ocr".to_string(),
        model_version: "fixture-1".to_string(),
        config_hash: "sha256:bound-config".to_string(),
    }
}

#[test]
fn receipt_carries_the_identity_and_input_hash() {
    let run = visible_text_run_from_analysis(&identity(), INPUT_HASH, &recorded_output()).unwrap();
    let id = identity();
    assert_eq!(run.receipt.modality, Modality::VisibleText);
    assert_eq!(run.receipt.producer, id.producer);
    assert_eq!(run.receipt.producer_version, id.producer_version);
    assert_eq!(run.receipt.model, id.model);
    assert_eq!(run.receipt.model_version, id.model_version);
    assert_eq!(run.receipt.config_hash, id.config_hash);
    assert_eq!(run.receipt.input_hash, INPUT_HASH);
}

#[test]
fn every_text_observation_maps_with_time_frame_language_region_and_confidence() {
    let run = visible_text_run_from_analysis(&identity(), INPUT_HASH, &recorded_output()).unwrap();
    assert_eq!(run.observations.len(), 6);
    let ids: BTreeSet<_> = run.observations.iter().map(|o| o.id.clone()).collect();
    assert_eq!(ids.len(), 6, "observation ids are unique within the run");
    let by_frame: BTreeMap<u64, _> = run
        .observations
        .iter()
        .map(|o| (o.frame_index.expect("frame index"), o))
        .collect();
    assert_eq!(
        by_frame.keys().copied().collect::<Vec<_>>(),
        vec![0, 2, 4, 5, 6, 8]
    );
    for (frame, observation) in &by_frame {
        assert!((observation.timestamp_seconds - *frame as f64 / 30.0).abs() < 1e-9);
        if *frame < 5 {
            assert_eq!(observation.text, "First Slide");
            assert_eq!(observation.language.as_deref(), Some("en"));
            assert_eq!(observation.confidence, Some(0.75));
            assert_eq!(observation.region, None);
        } else {
            assert_eq!(observation.text, "Second Slide");
            assert_eq!(observation.language, None);
            assert_eq!(observation.confidence, Some(0.5));
            assert_eq!(
                observation.region,
                Some(Region {
                    x: 0,
                    y: 0,
                    width: 2,
                    height: 1
                })
            );
        }
    }
}

#[test]
fn producer_tracks_map_with_exactly_their_member_observations() {
    let output = recorded_output();
    let run = visible_text_run_from_analysis(&identity(), INPUT_HASH, &output).unwrap();
    let frame_of: BTreeMap<String, u64> = run
        .observations
        .iter()
        .map(|o| (o.id.clone(), o.frame_index.unwrap()))
        .collect();
    assert_eq!(run.tracks.len(), output.text_semantics.tracks.len());
    for upstream in &output.text_semantics.tracks {
        let track = run
            .tracks
            .iter()
            .find(|track| track.id == upstream.id.as_str())
            .expect("one track per upstream track, with the upstream id");
        assert_eq!(track.text, upstream.text);
        let role = serde_json::to_value(upstream.role).unwrap();
        assert_eq!(track.role.as_deref(), role.as_str(), "upstream role name");
        let frames: BTreeSet<u64> = track
            .observation_ids
            .iter()
            .map(|id| frame_of[id])
            .collect();
        assert_eq!(track.observation_ids.len(), upstream.sample_count as usize);
        let expected: BTreeSet<u64> = if upstream.text == "First Slide" {
            [0, 2, 4].into()
        } else {
            [5, 6, 8].into()
        };
        assert_eq!(
            frames, expected,
            "{}: membership as upstream grouped it",
            track.text
        );
    }
}

#[test]
fn the_core_collapses_each_track_into_one_item() {
    let run = visible_text_run_from_analysis(&identity(), INPUT_HASH, &recorded_output()).unwrap();
    let analysis = compose(&CompositionInput {
        media: MediaIdentity {
            asset_id: AssetId::new("acceptance:issue-33-fixture").unwrap(),
            content_hash: ContentHash::from_bytes(b"fixture"),
            duration_seconds: Some(9.0 / 30.0),
        },
        speech: ModalityInput::NotRequested,
        visible_text: ModalityInput::Completed { runs: vec![run] },
        scenes: ModalityInput::NotRequested,
    })
    .unwrap();
    let items: Vec<_> = analysis
        .evidence
        .iter()
        .filter_map(|item| match &item.content {
            EvidenceContent::VisibleText(text) => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|text| text.observations.len() == 3));
}

#[test]
fn empty_output_maps_to_an_empty_run() {
    let empty = SceneAwareOcrAnalysis {
        sampled_frames: vec![0, 2, 4],
        observations: vec![],
        text_semantics: VideoTextSemanticAnalysis {
            tracks: vec![],
            annotations: vec![],
            relations: vec![],
        },
    };
    let run = visible_text_run_from_analysis(&identity(), INPUT_HASH, &empty).unwrap();
    assert!(run.observations.is_empty());
    assert!(run.tracks.is_empty());
    assert_eq!(run.receipt.input_hash, INPUT_HASH);
}

#[test]
fn a_text_observation_without_frame_or_timestamp_is_a_failure() {
    let mut output = recorded_output();
    output.observations[0].frame = None;
    output.observations[0].timestamp = None;
    match visible_text_run_from_analysis(&identity(), INPUT_HASH, &output) {
        Err(ProducerError::Failed { reason }) => assert!(!reason.trim().is_empty()),
        other => panic!("{other:?}"),
    }
}
