//! Real FFprobe 90 kHz VFR timestamps through the opt-in direct-file producer.

mod common;

use std::process::Command;

use common::*;
use media_intelligence_analysis::{
    analyze, analyze_with_exact_scene_timing, CachedStage, CancellationToken,
    ExactSceneAnalysisReport, InMemoryStageCache, Modality, Producers, SceneConfig, StageCache,
    StageStatus,
};
use media_intelligence_core::exact_timing::ExactAnchorTimingV1;

fn vfr_clip(nonzero_clock_origin: bool) -> Option<Clip> {
    if Command::new("ffmpeg").arg("-version").output().is_err()
        || Command::new("ffprobe").arg("-version").output().is_err()
    {
        eprintln!("SKIPPED 90 kHz VFR acceptance: ffmpeg and ffprobe are required");
        return None;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source-vfr.mp4");
    let mut command = Command::new("ffmpeg");
    command.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "lavfi",
        "-i",
        "color=c=red:s=64x64:r=10:d=0.2",
        "-f",
        "lavfi",
        "-i",
        "color=c=green:s=64x64:r=10:d=0.8",
        "-filter_complex",
        "[0:v][1:v]concat=n=2:v=1:a=0,select=not(eq(mod(n\\,4)\\,3))[v]",
        "-map",
        "[v]",
        "-fps_mode",
        "vfr",
        "-c:v",
        "libx264",
        "-threads",
        "1",
        "-pix_fmt",
        "yuv420p",
        "-video_track_timescale",
        "90000",
    ]);
    if nonzero_clock_origin {
        command.args(["-output_ts_offset", "1"]);
    }
    assert!(command.arg(&path).status().unwrap().success());
    let content_hash = file_hash(&path);
    Some(Clip {
        dir,
        path,
        content_hash,
    })
}

fn vfr_request(clip: &Clip) -> media_intelligence_analysis::AnalysisRequest {
    let mut profile = scenes_only();
    profile.scenes = Some(SceneConfig {
        threshold: 10.0,
        min_scene_len: 1,
    });
    request(clip, profile)
}

#[test]
fn direct_scene_decoding_preserves_the_original_90khz_vfr_pts_and_cache_provenance() {
    let Some(clip) = vfr_clip(false) else {
        return;
    };
    let request = vfr_request(&clip);
    let mut cache = InMemoryStageCache::new();
    let first = analyze_with_exact_scene_timing(
        &request,
        &Producers::default(),
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    let sidecar = first.exact_timing.as_ref().expect("exact scene sidecar");
    assert_eq!(sidecar.media, request.media);
    assert_eq!(sidecar.exact_modalities, vec![Modality::Scene]);
    assert_eq!(sidecar.anchors.len(), 2, "real red/green scene cut");
    let source = sidecar
        .anchors
        .iter()
        .map(|anchor| anchor.timing)
        .collect::<Vec<_>>();
    let [(start, cut), (next, end)] = source
        .iter()
        .map(|timing| match timing {
            ExactAnchorTimingV1::Range { start, end } => (start, end),
            other => panic!("scene must have a range, got {other:?}"),
        })
        .collect::<Vec<_>>()
        .try_into()
        .expect("two exact scene ranges");
    assert_eq!(start.pts, 0);
    assert_eq!(cut.pts, 18_000);
    assert_eq!(next.pts, 18_000);
    assert_eq!(end.pts, 90_000);
    for timestamp in [start, cut, next, end] {
        assert_eq!(
            (timestamp.timebase.num, timestamp.timebase.den),
            (1, 90_000)
        );
    }
    assert_eq!(first.legacy.analysis.evidence.len(), 2);
    assert!((first.legacy.analysis.evidence[0].end_seconds - 0.2).abs() < 1e-10);
    assert_eq!(sidecar.anchors[0].start_frame, Some(0));
    assert_eq!(sidecar.anchors[0].end_frame, Some(2));
    assert_eq!(sidecar.anchors[1].start_frame, Some(2));
    assert_eq!(sidecar.anchors[1].end_frame, Some(8));
    let serialized = serde_json::to_string(&first).unwrap();
    assert!(serialized.contains("\"pts\":\"18000\""));
    let round_trip: ExactSceneAnalysisReport = serde_json::from_str(&serialized).unwrap();
    assert_eq!(round_trip, first);
    assert_eq!(serde_json::to_string(&round_trip).unwrap(), serialized);

    // An exact hit keeps the original PTS; legacy analysis has a distinct
    // config hash/cache key and cannot silently reuse these exact anchors.
    let second = analyze_with_exact_scene_timing(
        &request,
        &Producers::default(),
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(second.legacy.scenes, StageStatus::Reused { .. }));
    assert_eq!(first.exact_timing, second.exact_timing);
    let legacy = analyze(
        &request,
        &Producers::default(),
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        legacy.scenes,
        StageStatus::CompletedWithFindings { .. }
    ));
    assert_eq!(cache.len(), 2);

    // An exact cache entry with corrupt original PTS must be re-decoded.
    let key = cache
        .keys()
        .into_iter()
        .find(|key| key.producer.config_hash != request.profile.scenes.unwrap().config_hash())
        .unwrap();
    let Some(CachedStage::ExactScenes { run, mut anchors }) = cache.get(&key) else {
        panic!("exact cache stores both run and anchors");
    };
    let ExactAnchorTimingV1::Range { start, end } = &mut anchors[0].timing else {
        panic!("range required");
    };
    start.pts = 1;
    end.pts = 18_001;
    cache.put(key, CachedStage::ExactScenes { run, anchors });
    let repaired = analyze_with_exact_scene_timing(
        &request,
        &Producers::default(),
        &mut cache,
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        repaired.legacy.scenes,
        StageStatus::CompletedWithFindings { .. }
    ));
    assert_eq!(repaired.exact_timing, first.exact_timing);
}

#[test]
fn direct_scene_decoding_rejects_unmapped_nonzero_stream_clock_origin() {
    let Some(clip) = vfr_clip(true) else {
        return;
    };
    let result = analyze_with_exact_scene_timing(
        &vfr_request(&clip),
        &Producers::default(),
        &mut InMemoryStageCache::new(),
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(result.exact_timing.is_none());
    match &result.legacy.scenes {
        StageStatus::Failed { reason, .. } => assert!(reason.contains("nonzero"), "{reason}"),
        other => panic!("unmapped source clock must not claim exact PTS: {other:?}"),
    }
}
