# Acceptance: issue #25 (compose aligned, source-grounded multimodal evidence)

The independent acceptance agent wrote these tests and fixtures **before** any
implementation existed. The public API in `src/lib.rs` and `src/compat/` is the
acceptance seam, and its bodies are `todo!("implementation pending (#25)")`.

## Spec sources

- media-intelligence issue #25 and its work-loop preflight comment (2026-10-14); goal issue #24.
- The README "Approved target direction" section at `4a675db`.
- youtube-corpus `src/media_evidence.rs` @ `0eb1c969a6b857183b5383af8d7c6e1e473263f1` (`MediaEvidenceBatchV1`).
- visual-analysis `video_text_semantics.rs` @ `115c1f0caceb644993df3121b4625e674a9f1b7a`. OCR tracking is owned there.
- moenarch-foundation @ `b0e26e4e671e5c43d659a2c67583ffdf5a9bbfa9`. These crates are exact-rev git dependencies:
  - `corpus-core`: `AssetId`, `ContentHash`;
  - `media-core`: `TimedTextContract`.
- scenedetect-rs `scenedetect-core` @ `adfcca6a`. Scene intervals are half-open frame spans.

## Tests

| File | Proves |
| --- | --- |
| `timeline_alignment.rs` | <ul><li>Speech, scenes and OCR share one timeline, in this order: start, end, modality (speech < visible_text < scene), id.</li><li>Time ranges are exact.</li><li>Every item carries the asset id, content hash and its own run's receipt.</li><li>Producer content passes through unchanged.</li><li>Transcript and OCR confidences stay at their exact producer values in modality-specific fields, and no item has a shared `score`/`confidence` field.</li></ul> |
| `relations.rs` | <ul><li>The exact expected set of scene→speech, scene→visible-text and speech↔visible-text relations.</li><li>Touching intervals and an instant at an interval's end are not related. An instant at an interval's start is related.</li><li>Relations are sorted, unique and reference existing items.</li></ul> |
| `ocr_tracks.rs` | <ul><li>4 repeated frames in one producer track become 1 item linked to all 4 observation ids.</li><li>Untracked repeats of the same text stay separate.</li><li>Without producer tracks, the core does no tracking of its own.</li></ul> |
| `coverage.rs` | <ul><li>not_requested, unprocessed, failed (with receipt and reason), completed_empty and completed_with_findings (with receipts and count) are all distinct.</li><li>A missing modality is never completed.</li><li>`Completed` with no run is rejected.</li></ul> |
| `revisions.rs` | Two OCR model versions with identical ids and content give 4 items, not 2. Each item keeps its own receipt and its own run's scores, and coverage lists both receipts. |
| `determinism.rs` | <ul><li>Output is byte-identical across repeated runs and after shuffling every input collection.</li><li>Changing the OCR config hash changes only OCR ids and the relations that touch them.</li><li>Changing the speech model version changes only speech ids.</li><li>Ids are bound to the media content hash.</li></ul> |
| `validation.rs` | Each of these returns an error rather than a panic: <ul><li>end < start;</li><li>a time before the media start;</li><li>NaN or infinite times;</li><li>a track naming an unknown observation;</li><li>a track naming an observation from another run (ids are run-scoped);</li><li>a receipt modality mismatch;</li><li>duplicate ids within a run;</li><li>an untimed timed-text segment.</li></ul> |
| `compatibility.rs` | <ul><li>The `MediaEvidenceBatchV1` fixture round-trips.</li><li>Conversion keeps scene, observation and track ids and every `ProcessingEvidenceV1` field (`runId` and `processingConfig` included).</li><li>Composition does not inflate OCR frames.</li><li>Empty batch lists become `unprocessed`.</li><li>A Foundation timed-text transcript plus the batch compose into one timeline with the expected co-occurrences.</li></ul> |

Run: `CARGO_BUILD_JOBS=2 cargo test --locked -p media-intelligence-core`.

## Decisions fixed by these tests (from the spec, resolved by the acceptance agent)

- **Interval semantics.** Items occupy `[start, end)`. An OCR observation is the instant `t`, which overlaps `[a, b)` iff `a <= t < b`. A track spans `[first, last]` of its member timestamps.
- **Producer-local ids are run-scoped.** Runs of one modality are never merged.
- **YouTube Corpus batches.**
  - The batch has no speech, so speech is `unprocessed`.
  - Empty scene or OCR lists cannot prove that the producer ran, so they are `unprocessed`, not `completed_empty`.
  - Rows are grouped into runs by their full provenance.
- **Content-addressed ids** include the media content hash, so a new content revision gives new ids.

## Not automated here

- That no producer algorithm or runtime is linked. This needs a review of `Cargo.lock`; the dependencies today are serde, serde_json, sha2 and the two Foundation crates.
- Real producer execution on a video. That is issue #27.
- Retrieval fusion. That is issue #26.

## Canonical producer output

None qualifies, so none is included and none was fabricated. See `tests/fixtures/README.md`.
The only real speech output found is a whisper.cpp transcript in video-analysis-studio. It was not copied because that repo has no license and the transcript reproduces copyrighted third-party speech.
