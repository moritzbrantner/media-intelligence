# Acceptance: issue #27, slice 1 (execution layer + canonical scene producer)

The independent acceptance agent wrote these tests **before** any implementation existed.
The public API in `src/lib.rs` is the acceptance seam. Its bodies are
`todo!("implementation pending (#27)")`.

## Spec sources

- media-intelligence issue #27 (body = behaviour authority) and its work-loop preflight
  comment of 2026-10-14, which defines slice 1. Goal issue #24.
- README "Approved target direction" at `b7db5da`.
- `media-intelligence-core` public API at `b7db5da` (#25): `compose`, `CompositionInput`,
  `ModalityInput`, `SceneRun`, `SpeechRun`, `VisibleTextRun`, `ProducerReceipt`,
  `EvidenceAnalysis`. This crate reuses these types and does not duplicate them.
- Canonical scene path: youtube-corpus `origin/main` `src/visual_processing.rs`
  (`FfmpegVideoSource::open` + `video_analysis_ingest::surface::detect_content_scenes`,
  threshold 27.0 and minimum scene length 15 by default, input re-hash after detection).
- visual-analysis `115c1f0caceb644993df3121b4625e674a9f1b7a`.

## Dependency graph

- `moenarch-video-analysis-ffmpeg` and `moenarch-video-analysis-ingest` are exact-rev git
  dependencies on visual-analysis `115c1f0c`.
- visual-analysis declares registry requirements on Foundation crates. Its
  `.coding-tooling.source-deps.json` names Foundation `b7533cd`. The workspace
  `[patch.crates-io]` resolves `moenarch-audio-contracts`, `-corpus-core`,
  `-math-geometry-2d`, `-media-core` and `-runtime-core` to Foundation `b0e26e4e`. The
  workspace already pins that revision; it descends from `b7533cd`, and the crate versions
  are identical. The graph holds one copy of each crate (`cargo tree -d` is empty), and the
  core's pins are unchanged.
- `scenedetect-core 0.1.0`, `moenarch-text-core 0.1.1` and its `moenarch-jobs-core 0.1.2`
  come from crates.io, as in visual-analysis's own `Cargo.lock`. Jobs-core was removed from
  Foundation at `b0e26e4`, so it cannot be patched to that revision.
- The scene producer calls the `ffmpeg` and `ffprobe` executables on `PATH`. No libav
  linkage, model, network or download is involved.

## Synthetic media, real producer

- **Media is synthetic.** Each test generates its own solid-colour H.264 clip into a temp
  dir with ffmpeg (`tests/common/mod.rs`). The main clip is red 5 s, green 7 s and blue 4 s
  at 25 fps. With ffmpeg 8.0.1 its sha256 is `03e3ba48…7279`, the same clip as the #25
  fixture. Tests compute the hash and do not pin it.
- **Scene execution is real.** `analyze` runs visual-analysis on that clip. The expected
  boundaries were checked by a throwaway probe against the pinned crates before the tests
  were written: [0, 5), [5, 12) and [12, 16) s, frames 0/125/300/400. With
  `min_scene_len = 200` there is one scene; a black/white clip has two.
- **Speech and visible-text producers are fakes** written by the acceptance agent (`FakeSpeech`,
  `FakeOcr` in `tests/common/mod.rs`). They implement the public producer traits and
  count their invocations. Their segments and observations were written by hand.
- If ffmpeg or ffprobe is not on `PATH`, the tests that need a clip print
  `SKIPPED <test>: ffmpeg/ffprobe not found …` and return. **CI must install ffmpeg**
  (the current `tests.yml` does not), or the real-execution tests prove nothing there.

## Tests

| File | Proves |
| --- | --- |
| `scene_execution.rs` | <ul><li>Scene receipt constants name visual-analysis `115c1f0c`, `scenedetect-core:content`, `0.1.0`.</li><li>`SceneConfig::default()` is 27.0 / 15. `config_hash()` is a deterministic `sha256:<64 hex>`, and changing either field changes it.</li><li>`input_hash()` is `sha256:<content hash>`.</li><li>A real run on the clip is `CompletedWithFindings(3)` with exact seconds and 0-based, end-exclusive frames.</li><li>The receipt names the producer, revision, model, config hash, input hash (= the clip's sha256) and the recorded threshold/minSceneLen.</li><li>The analysis equals `core::compose` of that scene run.</li><li>The scene config reaches the producer: `min_scene_len = 200` gives one scene [0, 16).</li></ul> |
| `availability.rs` | <ul><li>Speech and OCR requested without producers are `Unavailable`, with core coverage `Unprocessed` (never completed or empty), while scenes still complete.</li><li>A failing producer is `Failed` with its reason, in the report and in coverage. OCR and scenes still complete.</li><li>Failures are not cached: a rerun calls the failing producer again, and successful stages are `Reused`.</li><li>An empty run is `CompletedEmpty`, which is distinct from `Unavailable`.</li><li>A run whose receipt names a foreign input hash is `Failed` and is not composed.</li></ul> |
| `multimodal_timeline.rs` | <ul><li>Fake speech (4 segments, one crossing the 5 s cut) and fake OCR (4 frames, 2 in one producer track, so 3 items) share one timeline with the real scenes.</li><li>Every item carries the clip's asset id, content hash and input hash.</li><li>The exact set of 11 scene→speech, scene→OCR and speech↔OCR relations.</li><li>The analysis equals `core::compose` of the three runs.</li></ul> |
| `stage_reuse.rs` | <ul><li>An unchanged rerun with the same cache makes every stage `Reused` with its evidence count. Producers are not re-invoked (call counts), and the analysis, including every evidence id, is identical. Cache keys are pinned: modality + content hash + producer identity (scene identity = visual-analysis constants + config hash).</li><li>A planted cache entry (2 scenes) is reported instead of detection's 3, which proves the scene producer did not re-execute.</li><li>Changing the scene config re-executes scenes only (1 scene, new config hash, new scene ids). Speech and OCR are `Reused`, with unchanged ids.</li><li>Changing a provider's config hash re-executes only that provider. Scene ids are unchanged.</li><li>Different source bytes are never served from cache.</li></ul> |
| `cancellation.rs` | <ul><li>The token starts uncancelled, and clones share state.</li><li>Cancelled before the run: every requested stage is `Cancelled`, no producer is called, there is no evidence or relation, coverage is `Unprocessed`, and the cache stays empty.</li><li>The speech producer cancels during its run and returns `ProducerError::Cancelled`: speech is `Cancelled` (not failed), OCR is never invoked, and later stages are `Cancelled` with no evidence.</li><li>Not-requested stays `NotRequested` under cancellation.</li></ul> |
| `media_integrity.rs` | <ul><li>A missing file gives `Err(MediaMissing)`, and no producer runs.</li><li>Bytes that differ from the declared content hash give `Err(ContentHashMismatch { declared, actual })`, and no producer runs.</li><li>If bytes are replaced during the operation (by the speech producer), scenes are never completed or cached for the declared hash: either an error, or scenes `Failed` with no scene evidence.</li><li>An invalid scene config (NaN, negative threshold, zero min length) gives `Err(InvalidSceneConfig)` before any stage runs.</li></ul> |
| `not_requested.rs` | <ul><li>Supplied producers are not invoked when their modality is not requested, and coverage is `NotRequested`.</li><li>With scenes not requested, non-video bytes still analyse successfully for speech, which proves visual-analysis was not invoked.</li><li>An empty profile runs nothing and caches nothing.</li></ul> |

Run: `CARGO_BUILD_JOBS=2 cargo test --locked -p media-intelligence-analysis`. At the
acceptance commit it compiles, and 24 of 25 tests fail at `todo!` (the constants test
passes).

## Decisions fixed by these tests (resolved by the acceptance agent)

- **Stage order** is the `Modality` declaration order: speech, visible text, scenes. The
  mid-run cancellation and changed-bytes tests rely on speech running first.
- **Integrity is checked up front.** A missing file, a hash mismatch or an invalid scene
  config is a whole-call `Err`, not a per-stage status, and no producer runs.
- **Input hash convention**: `sha256:<ContentHash hex>`, matching the #25 scenedetect-rs
  fixture.
- **Frames**: visual-analysis frame indices are carried verbatim, 0-based and
  end-exclusive. The #25 scenedetect-rs CLI fixture uses 1-based inclusive frame numbers.
- **Producer receipts are validated.** A run must name its modality, its declared identity
  and the input hash; otherwise the stage is `Failed`.
- **Only successful runs are cached.** `Reused` carries the receipt and evidence count.
- **Status mapping to core coverage**: `Unavailable` and `Cancelled` map to `Unprocessed`;
  `Failed` maps to `Failed`; `Reused` and `Completed*` map to `Completed`.
- Not pinned: whether a cached stage may be reused after cancellation, and the exact
  `config_hash` input string.

## What slice 1 does not prove

- **No ASR or OCR model runs.** Speech and visible text come from acceptance fakes through
  the producer traits. The `audio-analysis` transcription adapter, the visual-analysis
  OCR + `analyze_video_text_semantics` adapter, and model-backed acceptance on a real
  video with speech and slide text are successor slices of #27.
- No real-world footage. The media is synthetic, and the scene detection on it is real.
- No interruption inside the visual-analysis scene run itself (cancellation is checked
  between stages and inside caller producers).
- Index/query fusion (#26) and consumer adoption.
