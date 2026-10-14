//! Shared facts about the committed #34 excerpt fixture and the tolerant matchers the
//! model-backed acceptance uses (`tests/model_acceptance_fixture.rs`,
//! `tests/model_backed_acceptance.rs`).
//!
//! Every constant here was observed by the independent acceptance author on the committed
//! bytes (see `tests/fixtures/model_acceptance/README.md`): slide windows from 1 fps contact
//! sheets and ffmpeg `select='gt(scene,0.05)'` change points, on-screen text read from the
//! frames. There is no reference transcript: speech is checked only against words that are
//! printed on the slides the narrator reads.

#![allow(dead_code)]

use std::path::PathBuf;

use media_intelligence_analysis::{
    AnalysisProfile, AnalysisRequest, AssetId, ContentHash, MediaIdentity, SceneConfig,
};

/// sha256 of the committed excerpt (`wcc-excerpt.webm`).
pub const FIXTURE_SHA256: &str = "c954e099a777f09b95ecd071146567672c4e25ea83448600adf6f2ea7240db8d";
/// sha256 of the original Wikimedia Commons file the excerpt was cut from.
pub const ORIGINAL_SHA256: &str =
    "29af9da70d0dee89b1f606dd4cd32e1c074c1a2e1aa0dda45c9bb6e6bbbadec4";
/// Asset id the acceptance declares for the excerpt (caller-owned identity).
pub const FIXTURE_ASSET_ID: &str = "acceptance:issue-34-wcc-excerpt";
/// Video duration of the excerpt: 800 frames at 25 fps (the container says 32.012 s).
pub const FIXTURE_DURATION_SECONDS: f64 = 32.0;
/// Container duration of the excerpt (audio may run to here).
pub const FIXTURE_CONTAINER_SECONDS: f64 = 32.012;

/// How far a detected scene boundary may sit from an observed visual change, and how far an
/// OCR observation time may sit outside a slide window and still count for that slide.
///
/// Justification: the excerpt's slide transitions are animated and last up to 0.52 s
/// (19.89 – 20.41 s, 8.09 – 8.41 s); a content detector may cut anywhere inside one; the
/// observed change points are frame-quantised (0.04 s at 25 fps). 0.52 + 0.04 rounded up
/// gives 0.6 s. Slides last 4.77 s or more, so 0.6 s cannot confuse two title changes.
pub const BOUNDARY_TOLERANCE_SECONDS: f64 = 0.6;

/// How far a speech segment may end after the excerpt's container duration. Whisper emits
/// timestamps in 0.02 s steps on 30 s windows and the excerpt's last window is padded;
/// anything beyond half a second is a timeline-mapping error, not rounding.
pub const SPEECH_END_TOLERANCE_SECONDS: f64 = 0.5;

/// One slide of the excerpt: its observed window (excerpt seconds) and content words of its
/// on-screen text (lower case, length >= 4, no generic words such as "step" or "your").
#[derive(Debug, Clone, Copy)]
pub struct Slide {
    pub name: &'static str,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub words: &'static [&'static str],
}

/// The four slides of the excerpt, in order.
pub const SLIDES: [Slide; 4] = [
    Slide {
        name: "Benefits of Creating an Account",
        start_seconds: 0.0,
        end_seconds: 4.77,
        // "Benefits of Creating an Account" / "Having an account allows you to save your
        // work and join the global Wikipedia community."
        words: &[
            "benefits",
            "creating",
            "account",
            "having",
            "allows",
            "global",
            "wikipedia",
            "community",
        ],
    },
    Slide {
        name: "Step 1: Go to Wikipedia & Click 'Create Account'",
        start_seconds: 4.77,
        end_seconds: 12.17,
        // Title, the "en.wikipedia.org" address bar, the "Welcome to Wikipedia" screenshot.
        words: &["wikipedia", "click", "create", "account", "welcome"],
    },
    Slide {
        name: "Step 2: Fill out the Form",
        start_seconds: 12.17,
        end_seconds: 19.89,
        // Title and the "Create account" form screenshot.
        words: &["fill", "form", "create", "account"],
    },
    Slide {
        name: "Step 3: Choosing the right username",
        start_seconds: 19.89,
        end_seconds: FIXTURE_DURATION_SECONDS,
        // Title, "Create account" / "Username" screenshot.
        words: &["choosing", "right", "username", "create", "account"],
    },
];

/// Title changes (excerpt seconds): Benefits -> Step 1, Step 1 -> Step 2, Step 2 -> Step 3.
pub const TITLE_CHANGES: [f64; 3] = [4.77, 12.17, 19.89];

/// Every visual change point ffmpeg `select='gt(scene,0.05)'` reports on the excerpt
/// (title changes, the address bar animation at 6.13, the screenshot transition 8.09 –
/// 8.41, the form scroll at 17.17, the Step 3 transition 19.89 – 20.41, "Dos" at 29.81).
/// Between these points the frames are visually near-static.
pub const VISUAL_CHANGES: [f64; 11] = [
    4.772, 6.132, 8.092, 8.252, 8.412, 12.172, 17.172, 19.892, 20.212, 20.412, 29.812,
];

/// Words printed on the excerpt's slides that the narration of these slides is expected to
/// say (whole words, case-insensitive). Used for speech only.
pub const SPEECH_VOCABULARY: [&str; 9] = [
    "wikipedia",
    "account",
    "accounts",
    "username",
    "usernames",
    "create",
    "creating",
    "form",
    "benefits",
];

/// Absolute path of the committed excerpt.
pub fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/model_acceptance/wcc-excerpt.webm")
}

/// Content hash of the committed excerpt, computed from its bytes.
pub fn fixture_content_hash() -> ContentHash {
    ContentHash::from_bytes(std::fs::read(fixture_path()).expect("committed excerpt fixture"))
}

pub fn fixture_media() -> MediaIdentity {
    MediaIdentity {
        asset_id: AssetId::new(FIXTURE_ASSET_ID).unwrap(),
        content_hash: fixture_content_hash(),
        duration_seconds: Some(FIXTURE_DURATION_SECONDS),
    }
}

pub fn fixture_request(profile: AnalysisProfile) -> AnalysisRequest {
    AnalysisRequest {
        media: fixture_media(),
        media_path: fixture_path(),
        profile,
    }
}

pub fn profile(speech: bool, visible_text: bool, scenes: bool) -> AnalysisProfile {
    AnalysisProfile {
        speech,
        visible_text,
        scenes: scenes.then(SceneConfig::default),
    }
}

/// Whether `ffmpeg` and `ffprobe` run (the scene producer and OCR frame sampling call them,
/// and audio-analysis decodes non-WAV media with them).
pub fn ffmpeg_available() -> bool {
    let runs = |program: &str| {
        std::process::Command::new(program)
            .arg("-version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    };
    runs("ffmpeg") && runs("ffprobe")
}

/// Lower-case alphanumeric tokens of `text` (everything else separates tokens).
pub fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Levenshtein distance over chars.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut current = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(ca != cb);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        previous = current;
    }
    previous[b.len()]
}

/// Whether an OCR token reads as `expected` allowing OCR noise: both have at least four
/// characters and their edit distance is at most a quarter of `expected`'s length (one
/// error for 4–7 letters, two for 8–11).
///
/// Justification: printed-text TrOCR typically has a character error rate of a few per
/// cent on clean renderings, so a correct read of a slide word is within this bound; a
/// 25 % budget still rejects unrelated words of the same length (any two distinct entries
/// of the slide vocabularies are farther apart, see `tests/model_acceptance_fixture.rs`).
///
/// A token at least as close to an [`OCR_DISTRACTORS`] word (text printed on every slide,
/// such as the "WIKIMEDIA FOUNDATION" footer, which is one edit from "wikipedia") as to
/// `expected` never matches, so the footer cannot pass for slide-specific text.
pub fn ocr_token_matches(observed: &str, expected: &str) -> bool {
    let (observed_len, expected_len) = (observed.chars().count(), expected.chars().count());
    if observed_len < 4 || expected_len < 4 {
        return false;
    }
    let distance = edit_distance(observed, expected);
    distance <= expected_len / 4
        && OCR_DISTRACTORS
            .iter()
            .all(|distractor| edit_distance(observed, distractor) > distance)
}

/// Text printed on every slide of the excerpt (footer logo), which therefore proves nothing
/// about which slide was read.
pub const OCR_DISTRACTORS: [&str; 2] = ["wikimedia", "foundation"];

/// The slide words that `text` contains, under [`ocr_token_matches`].
pub fn matched_words(text: &str, words: &[&'static str]) -> Vec<&'static str> {
    let observed = tokens(text);
    words
        .iter()
        .copied()
        .filter(|word| observed.iter().any(|token| ocr_token_matches(token, word)))
        .collect()
}

/// Slides whose window contains `seconds`, widened by [`BOUNDARY_TOLERANCE_SECONDS`] on
/// both sides (so a frame sampled right at a transition may count for either slide).
pub fn slides_at(seconds: f64) -> Vec<&'static Slide> {
    SLIDES
        .iter()
        .filter(|slide| {
            seconds >= slide.start_seconds - BOUNDARY_TOLERANCE_SECONDS
                && seconds < slide.end_seconds + BOUNDARY_TOLERANCE_SECONDS
        })
        .collect()
}

/// Whether `transcript` contains a [`SPEECH_VOCABULARY`] word as a whole token
/// (case-insensitive; no fuzzy matching for ASR output).
pub fn speech_vocabulary_hits(transcript: &str) -> Vec<&'static str> {
    let observed = tokens(transcript);
    SPEECH_VOCABULARY
        .iter()
        .copied()
        .filter(|word| observed.iter().any(|token| token == word))
        .collect()
}

/// Internal boundaries (start of every scene but the first) of sorted scene intervals.
pub fn internal_boundaries(intervals: &[(f64, f64)]) -> Vec<f64> {
    intervals.iter().skip(1).map(|(start, _)| *start).collect()
}

/// Whether some value in `points` is within [`BOUNDARY_TOLERANCE_SECONDS`] of `seconds`.
pub fn near_any(seconds: f64, points: &[f64]) -> bool {
    points
        .iter()
        .any(|point| (seconds - point).abs() <= BOUNDARY_TOLERANCE_SECONDS)
}
