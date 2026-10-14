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
  *Superseded by #33:* the order is now speech, scenes, visible text (see the #33 section,
  "Re-translated #27 contract").
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

# Acceptance: issue #32 (audio-analysis speech producer adapter)

The independent acceptance agent wrote these tests **before** any implementation existed.
The public API in `src/speech.rs` (feature `audio-analysis`) is the acceptance seam. Its
bodies are `todo!("implementation pending (#32)")`.

## Spec sources

- media-intelligence issue #32 (body) and its work-loop preflight comment of 2026-10-14.
- audio-analysis `cc8075f633a4bda5d1b9902b3f4c4d07eca017d9`,
  `crates/audio/audio-analysis-transcription/src/lib.rs`: `transcribe`,
  `TranscriptionPipelineRequest`, `TranscriptionProviderSelection` (with `provider_id()` and
  `model_id()`), `TranscriptionPipelineResponse`, and the Candle model resolution in
  `native_whisper.rs` (bundle, else cache-only, else Hugging Face download).
- The #27 slice 1 seam in `src/lib.rs` (`SpeechProducer`, `ProducerIdentity`, `analyze`,
  `StageCache`) and `media-intelligence-core`'s `SpeechRun::from_timed_text`.

## API under test

- Features: `audio-analysis` (off by default) adds the exact-rev git dependency
  `moenarch-audio-analysis-transcription` at `cc8075f6` with default features off.
  `audio-analysis-candle` also forwards audio-analysis `candle` for real native inference.
  No test needs it, and CI is not expected to build it.
- `speech::AUDIO_ANALYSIS_REVISION`, `SPEECH_PRODUCER_PREFIX` (`audio-analysis`) and
  `UNSPECIFIED_MODEL_VERSION` (`unspecified`).
- `speech::transcription` re-exports the pinned upstream crate.
- `ModelFetchPolicy { LocalOnly (default), AllowNetwork }`.
- `AudioAnalysisSpeechConfig { provider, vad, alignment, diarization, model_version,
  model_fetch }`, with `::new(provider)` and `config_hash()`.
- `AudioAnalysisSpeechProducer::new(config) -> Result<_, SpeechAdapterError>`, with
  `config()` and `transcription_request(media_path)`. It implements `SpeechProducer`.
- `SpeechAdapterError::ModelFetchNotAllowed { reason }`.
- `speech_run_from_response(identity, input_hash, response)` is a pure mapping.

## Tests (all gated by `#![cfg(feature = "audio-analysis")]`)

| File | Proves |
| --- | --- |
| `speech_adapter_identity.rs` | <ul><li>The revision constant is exactly `cc8075f6…`.</li><li>`new` defaults: default VAD, alignment and diarization off, no model version, `LocalOnly`.</li><li>Identity = `audio-analysis:<provider id>` / revision / model id / declared model version or `unspecified` / `config_hash()`, for candle-whisper, whisper-cpp and whisperx-command.</li><li>The hash is `sha256:<64 lowercase hex>` and deterministic for equal configs.</li><li>Each of 15 effective option changes changes the hash, and all 15 hashes differ: model id, language, task, compute type, device, model bundle, batch chunks, max batch size, decode runtime, VAD enabled/threshold/max chunk, alignment, diarization, provider kind.</li><li>The fetch permission and the declared model version do not change the hash. The version still changes the identity.</li><li>`transcription_request` passes source `Path { media_path }`, the provider, VAD, alignment and diarization through unchanged.</li></ul> |
| `speech_adapter_model_policy.rs` | <ul><li>Under `LocalOnly`, construction refuses these with `ModelFetchNotAllowed`: Candle without a bundle and not cache-only (also with only a `model_dir`), WhisperX without `model_cache_only`, cache-only WhisperX with alignment (`no_align: false`, the default) or `diarize: true`, and enabled alignment without a bundle and not cache-only.</li><li>Each reason names the local alternative.</li><li>Each refused config is accepted with `AllowNetwork`.</li><li>Cache-only and bundled configs, cache-only WhisperX with `no_align: true` and `diarize: false`, whisper.cpp, and disabled alignment are accepted without permission.</li></ul> |
| `speech_adapter_execution.rs` | <ul><li>On the real three-scene clip:<ul><li>A missing Candle bundle, a whisper.cpp `model_path` that is absent or missing, and a missing cache-only `model_dir` each give speech `Failed`. The reason names the option and the path. Coverage is `Failed`, and there is no speech evidence.</li><li>Scenes still complete with 3 scenes.</li><li>Nothing is cached for speech, and a rerun retries speech while scenes are `Reused`.</li></ul></li><li>An existing but empty bundle reaches audio-analysis. Its pipeline error makes speech `Failed` only.</li><li>Cancellation:<ul><li>A direct `run` with a cancelled token, a missing bundle and a missing media path is `Err(Cancelled)`, not `Failed`. This proves the token is checked before any file system or audio-analysis work.</li><li>A pre-cancelled `analyze` gives speech `Cancelled` and an empty cache.</li></ul></li><li>Reuse:<ul><li>A speech run planted under the adapter's identity is `Reused` with its receipt and 2 items, and it reruns identically. The bundle is missing, so an execution could only have failed.</li><li>Changing the language gives a new identity. Speech re-executes and fails, while scenes stay `Reused` with the same ids.</li></ul></li></ul> |
| `speech_adapter_mapping.rs` | <ul><li>A recorded `TranscriptionPipelineResponse` JSON in the upstream shape maps to exact segments: times, text, confidence 0.875/0.5/None, segment language with fallback to the transcript language, and speaker.</li><li>The receipt is `Speech` with every identity field and the input hash.</li><li>The result equals `SpeechRun::from_timed_text` of the same transcript.</li><li>An empty transcript gives an empty run.</li><li>These give `ProducerError::Failed`: `accepted: false` (the reason carries the diagnostics), a provider or model mismatch with the identity, and an untimed segment.</li></ul> |

Run: `CARGO_BUILD_JOBS=2 cargo test --locked -p media-intelligence-analysis --features audio-analysis`.
At the acceptance commit it compiles. 25 of the 26 #32 tests fail at `todo!`, and the
revision-constant test passes. The 30 pre-existing tests (#27) pass with and without the
feature.

## Dependency graph (#32)

- `moenarch-audio-analysis-transcription` 0.1.15 at audio-analysis `cc8075f6` is an
  exact-rev git dependency with default features off. It pulls
  `-speakers`, `-recognition` and `-core` from the same revision.
- audio-analysis requires exact registry versions of Foundation crates. Its source-deps file
  names Foundation `fb51ab4`, an ancestor of the pinned `b0e26e4` with identical versions.
  `[patch.crates-io]` now also covers `moenarch-math-signal-core`, `-model-runtime` and
  `-tensor-data`. `cargo tree -d` shows no duplicate `moenarch-*` crate. Third-party
  duplicates such as `getrandom` come from upstream.
- `audio-analysis-recognition` depends unconditionally on Foundation `model-runtime`. That
  links `hf-hub`/`ureq`/`rustls` into the feature build. The network capability is upstream
  code, which this adapter must not reach without `AllowNetwork`.
- The transcription crate has a `cmake` build dependency. Its build script does nothing
  unless the upstream `native` feature is on, and this crate does not enable `native`.

## Decisions fixed by these tests (resolved by the acceptance agent)

- **Permission is explicit and checked at construction.** `ModelFetchPolicy::AllowNetwork`
  is the only opt-in. The refusal is a constructor error, not a `Failed` stage. Missing
  model files are a run-time `Failed`, because files may appear between construction and
  run.
- **Download-capable** is defined from the upstream resolution code: no Candle bundle and
  not cache-only; WhisperX without cache-only; WhisperX with alignment (`no_align: false`)
  or `diarize: true` even when cache-only, because upstream WhisperX ignores
  `model_cache_only` for torchaudio alignment bundles and the pyannote diarization
  pipeline (whisperx `alignment.py` `load_align_model`, `transcribe.py`; Codex review on
  #37); or enabled native alignment without a bundle and not cache-only. whisper.cpp and
  native diarization (local bundles) never download.
- **Identity layout:** `producer` = `audio-analysis:<provider id>`, and `model` = the
  selection's model id. `model_version` is caller-declared, because audio-analysis reports
  none. It falls back to `unspecified`.
- **The config hash covers model locations** (bundle, dir and path). Different local
  weights under the same model id must not share cached results. A side effect is that
  cache keys are machine-specific. The hash excludes the fetch permission and the declared
  model version. The exact hash input string is not pinned.
- **Local model locations are pre-checked** by the adapter before audio-analysis is called,
  so the failure reason is actionable (option + path) rather than a downstream decode
  error.
- **Response guards:** `accepted: false`, or a provider or model id other than the identity,
  is `Failed`.

## What #32 acceptance does not prove

- **No ASR model is executed.** No test loads model weights or recognises words, and there
  are no model files on the test machine. Reused speech is a planted cache entry, and the
  mapping input is hand-written JSON. Real recognition from a local clip is the
  model-backed acceptance (#34).
- No network test exists. "Refused before any network attempt" is shown by construction
  failing before any run. Without `audio-analysis-candle`, the Candle download code is not
  compiled at all.
- The test clip has no audio track, so audio decoding of real media is not exercised.
  Whether the adapter enables audio-analysis `audio-io` for container decoding is left to
  the implementation and #34.
- Cancellation inside a running pipeline (the upstream observer's
  `cancellation_requested`) is not tested. Only cancellation before the run is.

# Acceptance: issue #33 (visual-analysis OCR + text-track producer adapter)

The independent acceptance agent wrote these tests **before** any implementation existed.
The acceptance seam is the #33 additions to `src/lib.rs` (`ProducerInput::scenes`,
`VisibleTextProducer::requires_scenes`, `scene_bound_identity`, the shared
`ModelFetchPolicy`) and `src/visible_text.rs` (feature `visual-analysis-ocr`). New bodies
are `todo!("implementation pending (#33)")`. `src/execution.rs` only gained `scenes: None`
so that the crate compiles; it still runs the #27 order.

## Spec sources

- media-intelligence issue #33 (body) and its work-loop preflight comment of 2026-10-14.
- visual-analysis `115c1f0caceb644993df3121b4625e674a9f1b7a` (read-only):
  - `crates/video/video-analysis-ingest/src/surface/scene_ocr.rs`:
    `analyze_scene_aware_ocr(source, backend, OcrRequest, video_id, scenes, duration)` →
    `SceneAwareOcrAnalysis { sampled_frames, observations, text_semantics }` (ingest
    feature `ocr`).
  - `crates/image/image-analysis-ocr/src/lib.rs`: `OcrBackend`, `OcrRequest`,
    `OcrLocalModelOptions` (`auto_download`), `OnnxTrOcrBackend::from_local_model` (real
    inference only with `local-onnx`), `JsonCommandOcrBackend` (PNG in, `OcrDocument` JSON
    out).
  - `crates/video/video-analysis-recognition/src/video_text_semantics.rs`:
    `analyze_video_text_semantics`, `VideoTextTrack`, `VideoTextRole`; and
    `model_analyzers.rs` `representative_scene_frames`, `ocr_document_observations`.
- The #27/#32 seam in `src/lib.rs` and `src/speech.rs`, and `media-intelligence-core`'s
  `VisibleTextRun`, `OcrObservation`, `OcrTrack`.

## API under test

- Core seam (all features):
  - Stage order speech → scenes → visible text. `ProducerInput::scenes:
    Option<&SceneRun>` hands the visible-text producer the scene run completed or reused by
    the same analysis; speech and scenes get `None`.
  - `VisibleTextProducer::requires_scenes()` (default `false`). For `true`: no scene run
    → stage `Failed` with a reason naming the scene stage, producer not invoked, no hidden
    scene detection; cache key and receipt use `scene_bound_identity(identity, scene_run)`.
  - `scene_bound_identity(identity, scenes)`: `identity` with `config_hash` replaced by a
    `sha256:<64 hex>` over its own config hash and the scene receipt identity.
  - `ModelFetchPolicy` moved to the crate root; `speech::ModelFetchPolicy` re-exports it.
- Feature `visual-analysis-ocr` (off by default) adds `visible_text`:
  - re-exports `ocr` (image-analysis-ocr), `video_core`, `recognition`,
    `SceneAwareOcrAnalysis`, `OcrRequest`, `OcrLocalModelOptions`, `OcrPreset`,
    `VISUAL_ANALYSIS_REVISION`;
  - constants `OCR_PRODUCER_PREFIX` (`visual-analysis`), `ONNX_TROCR_KIND`
    (`onnx-trocr`), `JSON_COMMAND_KIND` (`json-command`), `UNSPECIFIED_MODEL_VERSION`;
  - `OcrBackendConfig { OnnxTrOcr(OcrLocalModelOptions), JsonCommand { name, command, args } }`
    with `kind()` and `model_id()`;
  - `VisualAnalysisOcrConfig { backend, request, model_version, model_fetch }`, `::new`,
    `config_hash()`;
  - `VisualAnalysisOcrProducer::new(config) -> Result<_, OcrAdapterError>`, `config()`,
    implementing `VisibleTextProducer` with `requires_scenes() == true`;
  - `OcrAdapterError::ModelFetchNotAllowed { reason }`;
  - `visible_text_run_from_analysis(identity, input_hash, &SceneAwareOcrAnalysis)`, pure.
- Feature `visual-analysis-ocr-onnx` forwards image-analysis-ocr `local-onnx` (onnxruntime,
  tokenizers) for real ONNX TrOCR inference. No test needs it and it is not built here.

## Tests

| File | Proves |
| --- | --- |
| `stage_order.rs` (default features) | <ul><li>`scene_bound_identity` changes only `config_hash`, is a deterministic lowercase `sha256:`, and differs for another scene config hash or another own config hash.</li><li>A generic visible-text producer receives exactly the composed scene run (3 scenes, same receipt) and keeps its plain identity.</li><li>**Order:** a visible-text producer that cancels leaves speech and scenes completed (3 scenes); under the #27 order scenes would be `Cancelled`.</li><li>A reused scene run is handed over.</li><li>Without requested scenes a generic producer runs with `None`.</li><li>A scene-dependent producer: `Failed` with a reason naming scenes, not invoked and nothing cached, when scenes are not requested or the scene stage failed (non-video bytes); keyed (`StageKey`) and receipted under the bound identity, and reused on an unchanged rerun; re-executed when the scene config changes (different bound hash); a receipt with the unbound identity is rejected (`Failed`, scenes intact); pre-cancelled → `Cancelled` (with or without scenes).</li></ul> |
| `ocr_adapter_identity.rs` | <ul><li>Revision and kind constants.</li><li>`new` defaults: default `OcrRequest`, no version, `LocalOnly`.</li><li>`kind`/`model_id`: `json-command` + caller name; `onnx-trocr` + `Xenova/trocr-base-printed` / `-handwritten`.</li><li>Identity = `visual-analysis:<kind>` / revision / model id / declared version or `unspecified` / `config_hash()`, for both backends; `requires_scenes()` is true.</li><li>Hash: deterministic `sha256:<64 hex>`; each of 13 options changes it and all differ (JSON name, command, args; request languages, preserve_layout, include_tokens, min_confidence, attributes, technique; backend kind; ONNX preset, bundle_root, cache_dir).</li><li>Fetch permission, declared version and ONNX download settings (`auto_download`, `download_progress`, `hf_token`, `max_retries`) do not change it.</li></ul> |
| `ocr_adapter_model_policy.rs` | <ul><li>ONNX `auto_download` under `LocalOnly` → `ModelFetchNotAllowed`, reason names `auto_download` and `bundle_root`.</li><li>Accepted with `AllowNetwork`.</li><li>ONNX without `auto_download` (even with an `hf_token`) and a JSON command are accepted without touching the file system (paths do not exist).</li></ul> |
| `ocr_adapter_execution.rs` (ffmpeg + python3) | <ul><li>**End to end** on a red/green/blue slide clip (drawtext `RED SLIDE` etc.) with the real `JsonCommandOcrBackend` over a scripted command: scenes 3; visible text `CompletedWithFindings(3)`; receipt = bound identity (`visual-analysis:json-command`, revision, `acceptance-scripted-ocr`, `script-1`) + the clip's input hash; **each slide is one item from a producer track that holds every observation of its scene** (≥ 3, frames inside the scene, observation total = command calls); confidence 0.9; items lie inside their scene on the shared timeline; scene→text relations are exactly scene#k → its slide; the run is cached under the bound `StageKey`; the analysis equals `core::compose` of the scene run and the cached run.</li><li>Unchanged rerun: both stages `Reused`, the command is not called again, identical analysis. An OCR request change: new identity, only OCR re-runs (calls double), scene ids unchanged.</li><li>Missing ONNX `bundle_root` → OCR `Failed` naming `bundle_root` and the path, coverage `Failed`, no OCR evidence, only the scene run cached; rerun retries OCR, scenes `Reused`. An existing but empty bundle root → OCR `Failed`, scenes complete.</li><li>Scenes not requested → OCR `Failed` naming scenes; command never runs; cache empty.</li><li>Direct `run` without a scene run → `Failed` naming scenes, no command call. Direct `run` with a cancelled token, missing media, missing bundle and no scenes → `Err(Cancelled)`. Pre-cancelled `analyze` → OCR and scenes `Cancelled`, no command call, empty cache.</li></ul> |
| `ocr_adapter_mapping.rs` | <ul><li>Input: real upstream `analyze_scene_aware_ocr` over fixture frames and a fixture `OcrBackend` (visual-analysis' own two-scene scenario; preconditions: sampled 0,2,4,5,6,8; 2 tracks × 3 samples).</li><li>Receipt: `VisibleText`, every identity field, input hash.</li><li>6 observations with unique ids: text, time = frame/30, frame index, language `en` from the document, confidence 0.75 / 0.5 from the upstream score, region `Region {0,0,2,1}` from the block.</li><li>One `OcrTrack` per upstream track: upstream id, consensus text, role = upstream serde name, `observation_ids` = exactly the members (frames 0,2,4 and 5,6,8), count = `sample_count`.</li><li>Composed through the core: 2 items with 3 observations each.</li><li>Empty output → empty run. A text observation with neither frame nor timestamp → `Failed`.</li></ul> |

`tests/ocr_common/mod.rs` generates the slide clip and writes the scripted command. The
script reads the first pixel of the frame PNG (the scene background) and prints a fixed
`OcrDocument` (`RED SLIDE`/`GREEN SLIDE`/`BLUE SLIDE`, confidence 90, language `en`); each
call appends to a log, which the tests count. Without ffmpeg, ffprobe or python3 the
execution tests print `SKIPPED …` and return; without a working `drawtext` the clip has
solid scenes (the script never reads the drawn text). A throwaway probe confirmed the slide
clip's scenes are [0,5), [5,12), [12,16) s (frames 0/125/300/400), like the #27 clip.

Run: `CARGO_BUILD_JOBS=2 cargo test --locked -p media-intelligence-analysis --features visual-analysis-ocr`.
At the acceptance commit it compiles. Of the 36 #33 tests, 3 pass already: the
`ocr_adapter_identity` constants test, and in `stage_order` the generic `None` hand-off and
the pre-cancelled scene-dependent case, which hold under #27 behaviour. The other adapter
tests fail at `todo!`, and the other `stage_order` seam tests fail at `todo!`
(`scene_bound_identity`) or at their assertions, because `execution.rs` still runs the #27
order and passes `scenes: None`. The 30 #27 tests pass with default features, with
`audio-analysis` (plus the 31 #32 tests) and with `visual-analysis-ocr`.

## Re-translated #27 contract

Independent re-translation of the #27 stage-order decision ("`Modality` declaration order:
speech, visible text, scenes") to speech → scenes → visible text, because OCR samples from
the scene intervals (preflight of #33). Checked every #27 and #32 test:

- **No #27 test asserted that visible text runs before scenes.** The order-dependent tests
  (`cancellation_during_a_stage_stops_later_stages`, the changed-bytes test in
  `media_integrity.rs`, `cached_stage_is_not_reused_after_an_earlier_producer_changed_the_media`)
  depend only on speech running first, which still holds. They are unchanged and pass.
- `tests/cancellation.rs`: only the comment "Stages run in modality order" was rewritten to
  name the new order.
- `tests/speech_adapter_execution.rs`: the one struct literal of `ProducerInput` gained
  `scenes: None` (mechanical, needed to compile).
- The new order is pinned positively by `stage_order.rs`
  (`cancellation_by_the_visible_text_producer_keeps_the_scenes_that_already_ran`,
  `visible_text_producer_receives_the_scene_run_completed_by_the_same_analysis`).
- The #27 decision bullet above is marked superseded; the `lib.rs` contract docs name the
  new order.

## Dependency graph (#33)

- New exact-rev git dependencies at visual-analysis `115c1f0c` (optional, feature
  `visual-analysis-ocr`): `moenarch-image-analysis-ocr`, `-video-analysis-core`,
  `-video-analysis-recognition`; the feature also enables `video-analysis-ingest/ocr` and
  `video-analysis-recognition/ocr`. Dev-dependencies: `moenarch-image-analysis-core` (only
  for the fixture `OcrBackend`'s `ImageView`) and `num-rational` 0.4.2 (frame rates).
- `[patch.crates-io]` now also resolves `moenarch-runtime-onnx`, `-semantic-core`,
  `-math-linear` and `-vector-analysis-core` to Foundation `b0e26e4` (identical versions to
  the registry requirements). `cargo tree -d` with `audio-analysis,visual-analysis-ocr`
  shows no duplicate `moenarch-*` crate; third-party duplicates (`sha2`/`digest` family,
  `getrandom`, now also `miniz_oxide`) come from upstream.
- `Cargo.lock` records `ort`/`ort-sys` (behind `runtime-onnx`'s `onnxruntime` feature) and
  `tokenizers`; neither is compiled without `visual-analysis-ocr-onnx`.
- `image-analysis-ocr` depends on Foundation `model-runtime` (Hugging Face download code).
  Without `local-onnx`, `OnnxTrOcrBackend::from_local_model` returns a feature error before
  any resolution; with it, only `auto_download` enables downloads, which the adapter refuses
  under `LocalOnly`.

## Decisions fixed by these tests (resolved by the acceptance agent)

- **Scene hand-off is a core seam**, not adapter-private: `ProducerInput::scenes` plus
  `requires_scenes()`. The "no scene run" failure is decided by the core for scene-dependent
  producers (the producer is not invoked), and by the adapter itself on a direct `run`.
  Cancellation wins over that failure.
- **Scene-bound identity (consequential).** A cached OCR result is only valid for the scene
  intervals it sampled. Rather than add a field to `StageKey` (which would break the
  #27/#32 key literals), the scene receipt identity is folded into the visible-text
  `config_hash` of scene-dependent producers, in both the cache key and the receipt. So the
  OCR receipt `config_hash` differs from `VisualAnalysisOcrConfig::config_hash()`; the
  latter is the adapter's own `identity().config_hash`. A scene-config change re-runs OCR;
  an OCR-config change re-runs only OCR.
- **Identity layout:** `producer` = `visual-analysis:<onnx-trocr|json-command>`;
  `model` = the ONNX preset's repo id or the JSON command's caller-chosen `name`;
  `model_version` caller-declared, else `unspecified`.
- **Config hash** covers JSON name/command/args, ONNX preset/`bundle_root`/`cache_dir`, the
  whole `OcrRequest` and the backend kind; it excludes the fetch permission, the declared
  version and the ONNX download settings (`hf_token` must never enter a hash or receipt).
  Upstream sampling has no options at `115c1f0c`. The exact hash input is not pinned.
- **Download-capable** = ONNX TrOCR with `auto_download`. A JSON command is the caller's own
  executable and is never refused (the adapter cannot vet it). A missing or empty model
  bundle is a run-time `Failed`, never a construction error.
- **Observation ids** are adapter-assigned, unique within the run; their format is not
  pinned. Confidence is the upstream observation score (OCR confidence / 100) unchanged;
  `role` is the upstream `VideoTextRole` serde name.
- Not pinned: behaviour for a completed scene run with zero scenes; whether the
  scene-dependent `Failed` status carries an attempted receipt.

## Upstream API gap (blocks a faithful implementation)

At `115c1f0c`, `VideoTextTrack` reports `sample_count`, frame/time bounds and scene indices
but **not which observations it contains**, and `SceneAwareOcrAnalysis` adds no membership
either (`analyze_video_text_semantics` groups samples privately in `associate_samples`).
The core collapses repeated frames only through `OcrTrack::observation_ids`, and
`ocr_adapter_mapping.rs` / `ocr_adapter_execution.rs` require exact membership. The adapter
must not reconstruct membership itself (that would be tracking code in this repository).
Implementation therefore needs a focused visual-analysis change that exposes per-track
member observations (for example observation indices on `VideoTextTrack` or a track id on
each output observation) and a revision bump here. If that change adds a field to
`SceneAwareOcrAnalysis`, the `empty_output_maps_to_an_empty_run` struct literal needs the
new field (mechanical).

## What #33 acceptance does not prove

- **No OCR model runs.** The JSON backend calls an acceptance script that answers from the
  frame's background colour; it does not read the drawn text. ONNX TrOCR is only exercised
  without model files. Real recognition of slide text from a local clip is #34.
- No network test. "No surprise downloads" is shown by refusal at construction.
- Cancellation inside a running upstream OCR pass is not tested (upstream offers no
  cancellation hook); only cancellation before the run is.
- Upstream sampling density and tracking heuristics are trusted, not re-tested; the tests
  assert only start/middle/last coverage per scene and the resulting grouping.
