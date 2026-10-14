# Acceptance fixtures (#25)

All JSON fixtures here were **authored by the acceptance agent** from the issue #25
specification on 2026-10-14. They are not producer output. Identifiers, hashes and
receipts are made to look like the real seams (YouTube Corpus `ProcessingEvidenceV1`
values such as `visual-analysis` / `scenedetect-core:content` / `fnv1a64:` hashes,
Foundation timed text), but every value was written by hand or by a small script.

| File | Shape | Purpose |
| --- | --- | --- |
| `lecture_composition.json` | `CompositionInput` | Speech + 3 scenes + 8 OCR frames (2 producer tracks, 2 untracked repeats); touching-interval cases at t=10, 12, 18 |
| `partial_coverage.json` | `CompositionInput` | Failed speech (with receipt + reason), unprocessed OCR, completed-empty scenes |
| `conflicting_ocr_revisions.json` | `CompositionInput` | Two OCR runs with different model versions and identical producer-local ids/content |
| `youtube_corpus_media_evidence_batch_v1.json` | YouTube Corpus `MediaEvidenceBatchV1` (`src/media_evidence.rs` at youtube-corpus `0eb1c969a6b857183b5383af8d7c6e1e473263f1`) | Compatibility: scenes, OCR observations and one OCR track with full provenance |
| `timed_text_transcript.json` | Foundation `TimedTextContract` (moenarch-foundation `b0e26e4e671e5c43d659a2c67583ffdf5a9bbfa9`, the shape `audio-analysis` emits via `audio-timed-text-compat`) | Speech for the same video as the batch |

## Canonical producer output: gap

No recorded output of a canonical producer (scene detection, speech recognition, OCR) on
**real media** that can be redistributed here exists in the owner repositories, so none
is included and none was fabricated. Searched on 2026-10-14 (origin/main):

- `scenedetect-rs` `adfcca6a`: `site/data/quality.json` is real PySceneDetect 0.7
  (`scenedetect-headless==0.7`) reference output, but only cut frames for 8
  ffmpeg-lavfi **synthetic** 64x64 clips. `site/data/benchmarks*.json` names real-video
  cases but records timings only ("media is not committed").
- `native-whisperx` `424f6504`: the WhisperX 3.8.6 parity expectations
  (`expected/whisperx-3.8.6/*.json`) live under an external `PARITY_SMOKE_ROOT` on a
  self-hosted runner. They are not committed. `tests/fixtures/whisperx-parity-sample.json`
  is hand-written.
- `visual-analysis` `115c1f0c`: the OCR fixtures are rendered input images. Its README
  forbids replacing them with generated model output, and no OCR output is recorded.
- `youtube-corpus` `0eb1c969`, `moenarch-foundation` `b0e26e4e`, `media-similarity`
  `b0f0f1f5`: only inline or hand-written fixtures.
- `audio-analysis` `cc8075f6`: the Mixxx 2.5.4 baseline is real but holds aggregate
  beat/key agreement only. It has no speech, scene or OCR output.
- `video-analysis-studio` `dfe031b7`:
  - `fixtures/workflow-output/subtitle-generation/` holds a whisper.cpp `ggml-base.en`
    transcript of a third-party YouTube video (153 segments, no version pin). It is the
    only real speech output found, but it was **not copied**: the repo has no LICENSE
    file and the transcript reproduces copyrighted third-party speech.
  - The red-car reports use the in-house detector, not a canonical producer.

Real producer execution on an available video is the scope of issue #27.
