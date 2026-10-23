//! #34: cheap, always-built checks of the committed model-acceptance fixture. **No model
//! runs here.** These tests pin the fixture bytes and attribution, run the real canonical
//! scene producer on the excerpt (default features), and check the tolerant matchers the
//! opt-in model-backed acceptance (`tests/model_backed_acceptance.rs`) relies on.

mod model_acceptance_common;

use media_intelligence_analysis::{
    analyze, input_hash, CancellationToken, InMemoryStageCache, Producers, StageStatus,
    SCENE_MODEL, SCENE_PRODUCER,
};
use media_intelligence_core::EvidenceContent;
use model_acceptance_common::*;

#[test]
fn committed_excerpt_has_the_recorded_hash_and_size_budget() {
    let bytes = std::fs::read(fixture_path()).expect("committed excerpt");
    assert!(
        bytes.len() <= 1_500_000,
        "fixture size budget: {}",
        bytes.len()
    );
    assert_eq!(fixture_content_hash().as_str(), FIXTURE_SHA256);
}

#[test]
fn attribution_names_source_author_licence_modification_and_hashes() {
    let readme = std::fs::read_to_string(
        fixture_path()
            .parent()
            .expect("fixture dir")
            .join("README.md"),
    )
    .expect("fixture attribution README");
    for required in [
        "Wikimedia Core Curriculum - Initial steps",
        "Aespinoza09",
        "https://commons.wikimedia.org/wiki/File:Wikimedia_Core_Curriculum_-_Initial_steps.webm",
        "CC BY 4.0",
        "https://creativecommons.org/licenses/by/4.0/",
        "Excerpt, re-encoded",
        "ffmpeg -hide_banner",
        FIXTURE_SHA256,
        ORIGINAL_SHA256,
    ] {
        assert!(
            readme.contains(required),
            "attribution README lacks `{required}`"
        );
    }
}

#[test]
fn ocr_matcher_tolerates_noise_but_not_other_words_or_the_footer() {
    assert!(ocr_token_matches("wikipedia", "wikipedia"));
    assert!(ocr_token_matches("wikipedla", "wikipedia"));
    assert!(ocr_token_matches("acc0unt", "account"));
    assert!(ocr_token_matches("usemame", "username"));
    // The footer printed on every slide must not pass for "Wikipedia".
    assert!(!ocr_token_matches("wikimedia", "wikipedia"));
    assert!(!ocr_token_matches("foundation", "fill"));
    // Short tokens never count.
    assert!(!ocr_token_matches("from", "form"));
    assert!(!ocr_token_matches("for", "form"));
    // No two distinct vocabulary words of any slide can be confused with each other.
    let mut words: Vec<&str> = SLIDES
        .iter()
        .flat_map(|slide| slide.words)
        .copied()
        .collect();
    words.sort_unstable();
    words.dedup();
    for a in &words {
        for b in &words {
            if a != b {
                assert!(!ocr_token_matches(a, b), "`{a}` would read as `{b}`");
            }
        }
    }
    assert_eq!(
        matched_words("Step 2: Fill out the Form", SLIDES[2].words),
        vec!["fill", "form"]
    );
    assert_eq!(
        speech_vocabulary_hits("You can create an Account on Wikipedia."),
        vec!["wikipedia", "account", "create"]
    );
    assert!(speech_vocabulary_hits("accountant formal").is_empty());
}

#[test]
fn slide_table_covers_the_excerpt_without_gaps() {
    assert_eq!(SLIDES[0].start_seconds, 0.0);
    assert_eq!(SLIDES[3].end_seconds, FIXTURE_DURATION_SECONDS);
    for pair in SLIDES.windows(2) {
        assert_eq!(pair[0].end_seconds, pair[1].start_seconds);
    }
    for (change, slide) in TITLE_CHANGES.iter().zip(&SLIDES[1..]) {
        assert_eq!(*change, slide.start_seconds);
        assert!(near_any(*change, &VISUAL_CHANGES));
    }
}

/// The real visual-analysis scene producer on the excerpt: at least two scenes, cuts only
/// where the frames visibly change, and at least one title change found.
///
/// Observed with the default configuration (threshold 27, minimum 15 frames) at the pinned
/// revision: [0, 8.40), [8.40, 19.88), [19.88, 32.0). The Benefits -> Step 1 (4.77 s) and
/// Step 1 -> Step 2 (12.17 s) title changes keep the same light background and layout and
/// change only the title text, which stays under the content threshold; the full-frame
/// screenshot transition (8.09 – 8.41 s) and the Step 3 transition (19.89 – 20.41 s) are
/// cut. The assertions below hold for that run without pinning it exactly.
#[test]
fn real_scene_detection_on_the_excerpt_finds_the_slide_changes() {
    if !ffmpeg_available() {
        eprintln!(
            "SKIPPED real_scene_detection_on_the_excerpt_finds_the_slide_changes: \
             ffmpeg/ffprobe not found on PATH"
        );
        return;
    }
    let request = fixture_request(profile(false, false, true));
    let mut cache = InMemoryStageCache::new();
    let report = analyze(
        &request,
        &Producers::default(),
        &mut cache,
        &CancellationToken::new(),
    )
    .expect("analysis of the committed excerpt");

    let receipt = match &report.scenes {
        StageStatus::CompletedWithFindings {
            receipt,
            evidence_count,
        } if *evidence_count >= 2 => receipt.clone(),
        other => panic!("expected at least two scenes, got {other:?}"),
    };
    assert_eq!(receipt.producer, SCENE_PRODUCER);
    assert_eq!(receipt.model, SCENE_MODEL);
    assert_eq!(receipt.input_hash, input_hash(&request.media.content_hash));

    let mut intervals: Vec<(f64, f64)> = report
        .analysis
        .evidence
        .iter()
        .filter(|item| matches!(item.content, EvidenceContent::Scene(_)))
        .map(|item| (item.start_seconds, item.end_seconds))
        .collect();
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    eprintln!("excerpt scenes (s): {intervals:?}");

    assert!(intervals[0].0.abs() <= 0.05, "first scene starts at 0");
    let last_end = intervals.last().unwrap().1;
    assert!(
        (last_end - FIXTURE_DURATION_SECONDS).abs() <= 0.1,
        "last scene ends at the excerpt end, got {last_end}"
    );
    let boundaries = internal_boundaries(&intervals);
    for boundary in &boundaries {
        assert!(
            near_any(*boundary, &VISUAL_CHANGES),
            "cut at {boundary} s where the frames are visually static"
        );
    }
    let found = TITLE_CHANGES
        .iter()
        .filter(|change| near_any(**change, &boundaries))
        .count();
    assert!(
        found >= 1,
        "at least one of the title changes {TITLE_CHANGES:?} must be a scene boundary; \
         got boundaries {boundaries:?}"
    );
}
