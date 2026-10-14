# Acceptance fixtures (#25)

Except for the scenedetect-rs run described below, all JSON fixtures here were **authored by the acceptance agent** from the issue #25
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
| `scenedetect_rs_three_scenes.scenes.json` + `.provenance.json` | scenedetect-rs `list-scenes --format json` output (verbatim) + recorded provenance | Actual canonical scene-producer run, see below |
| `timed_text_transcript.json` | Foundation `TimedTextContract` (moenarch-foundation `b0e26e4e671e5c43d659a2c67583ffdf5a9bbfa9`, the shape `audio-analysis` emits via `audio-timed-text-compat`) | Speech for the same video as the batch |

## Canonical producer output: actual scenedetect-rs run (synthetic media)

`scenedetect_rs_three_scenes.scenes.json` is the **verbatim, unedited output of a real
execution** of the canonical scene producer, scenedetect-rs. The **media is synthetic**:
an ffmpeg-generated clip of three solid-colour segments, not real-world footage. The
producer execution is real. The clip itself is not committed; its hash is recorded.
Run on 2026-10-14 by the acceptance agent:

- Producer: scenedetect-rs `scenedetect-cli` 0.1.0 at public revision
  `adfcca6aa3a0bfdc0d12ecfd8f9f6db3284a28bd`, built with
  `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=<scratch>/sdtarget cargo install --locked --git https://github.com/moritzbrantner/scenedetect-rs --rev adfcca6aa3a0bfdc0d12ecfd8f9f6db3284a28bd --root <scratch>/sd scenedetect-cli`
  (rustc 1.99.0). The CLI has no `--version` flag.
- Media generator: `ffmpeg version 8.0.1-3ubuntu2`:
  `/usr/bin/ffmpeg -hide_banner -loglevel error -y -f lavfi -i "color=c=red:s=320x240:r=25:d=5" -f lavfi -i "color=c=green:s=320x240:r=25:d=7" -f lavfi -i "color=c=blue:s=320x240:r=25:d=4" -filter_complex "[0:v][1:v][2:v]concat=n=3:v=1:a=0[v]" -map "[v]" -c:v libx264 -threads 1 -pix_fmt yuv420p -bitexact -fflags +bitexact three-scenes.mp4`
- Clip: `three-scenes.mp4`, 14660 bytes. sha256
  `03e3ba48bef6b67535d18c24a1c8e7dbfaf0f9fad9b17dcd1918add385ba7279`. ffprobe reports
  25/1 fps, 400 frames, 16.000 s.
- Producer command: `scenedetect-rs -i three-scenes.mp4 --output out detect-content list-scenes --format json`
  (default detect-content options). It exited 0 and wrote `out/scenes.json`, copied here
  byte-for-byte: sha256 `f2b1a728ed7c114e27751978e8946a09f614d46c4ed408656e2f982270427953`,
  no trailing newline.
- Result: 3 scenes, [0, 5), [5, 12) and [12, 16) s. Frame numbers are the producer's
  1-based inclusive `list-scenes` numbering (1–125, 126–300, 301–400) and are carried
  verbatim.
- `scenedetect_rs_three_scenes.provenance.json` records the above as a `ProducerReceipt`.
  `configHash` is the sha256 of the literal string in `config.configHashInput`; the
  acceptance agent chose that convention, and scenedetect-rs does not emit it.
- `tests/canonical_scene_producer.rs` maps the output into a `SceneRun` **in test code
  only**, so there is no production adapter. It then composes the run alone and together
  with `timed_text_transcript.json`.

## Recorded producer output on real-world media: still a gap

No redistributable recorded output of a canonical producer on **real-world media**
exists in the owner repositories, so none is included and none was fabricated. Searched on 2026-10-14 (origin/main):

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
