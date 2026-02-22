# Service Spec: Transcription

## Responsibility

Convert audio or video files into time-stamped, speaker-labeled transcript segments using WhisperX with pyannote speaker diarization.

---

## Interface

### Input

Consumes from `queue: media.ingested`.

Expects a `MediaObject` (or reference to one in MinIO) where at least one `media_file` has `type: audio` or `type: video`.

### Output

A list of `TranscriptSegment` objects stored at `minio://processed/{media_id}/transcript.json`.

### Publishes To

`queue: content.extracted`

---

## Processing Steps

1. **Fetch audio** from MinIO (`audio.m4a` or extract from video using ffmpeg)
2. **Detect language** if not already known (WhisperX can auto-detect)
3. **Run WhisperX transcription** with word-level timestamps
4. **Run pyannote diarization** to detect speaker turns
5. **Align speakers to transcript segments** (merge diarization output with word-level timestamps)
6. **Normalize segment boundaries** — split on silence gaps, maximum 60 seconds per segment
7. **Store results** as `transcript.json` in MinIO
8. **Publish** `content.extracted` event

---

## Technology

### WhisperX

WhisperX wraps OpenAI Whisper with:
- Forced alignment for word-level timestamps (using wav2vec2)
- Faster inference via CTranslate2 backend
- Batch processing support

**Recommended model**: `large-v2` for quality, `medium` for speed.

```python
import whisperx

model = whisperx.load_model("large-v2", device="cuda", compute_type="float16")
audio = whisperx.load_audio(audio_path)
result = model.transcribe(audio, batch_size=16)

# Align
align_model, metadata = whisperx.load_align_model(language_code=result["language"], device="cuda")
result = whisperx.align(result["segments"], align_model, metadata, audio, device="cuda")
```

### pyannote.audio — Speaker Diarization

```python
from pyannote.audio import Pipeline

pipeline = Pipeline.from_pretrained(
    "pyannote/speaker-diarization-3.1",
    use_auth_token="HF_TOKEN"
)
diarization = pipeline(audio_path)
```

**Note**: pyannote requires a HuggingFace token and acceptance of the model license. Store the token in `HF_TOKEN` environment variable.

### Merging Diarization with Transcript

After diarization, assign a `speaker_id` to each transcript segment by finding the speaker that overlaps most with the segment's time range:

```python
def assign_speakers(segments, diarization):
    for segment in segments:
        max_overlap = 0
        assigned_speaker = "UNKNOWN"
        for turn, _, speaker in diarization.itertracks(yield_label=True):
            overlap = min(turn.end, segment["end"]) - max(turn.start, segment["start"])
            if overlap > max_overlap:
                max_overlap = overlap
                assigned_speaker = speaker
        segment["speaker_id"] = assigned_speaker
    return segments
```

---

## Output Format

```json
[
  {
    "media_id": "youtube_abc123def456",
    "segment_index": 0,
    "speaker_id": "SPEAKER_00",
    "speaker_name": null,
    "start_seconds": 0.0,
    "end_seconds": 12.4,
    "text": "Welcome to the show. Today we're talking about Bitcoin.",
    "language": "en",
    "confidence": 0.97
  },
  {
    "media_id": "youtube_abc123def456",
    "segment_index": 1,
    "speaker_id": "SPEAKER_01",
    "speaker_name": null,
    "start_seconds": 12.8,
    "end_seconds": 34.1,
    "text": "Thanks for having me. I've been following this space since 2011.",
    "language": "en",
    "confidence": 0.94
  }
]
```

---

## Performance Considerations

- GPU is required for reasonable performance. CPU-only will work but is 10–20x slower.
- A 1-hour video takes approximately 3–5 minutes on an A10G GPU with `large-v2`
- Audio should be converted to 16kHz mono WAV before processing for best results:
  ```bash
  ffmpeg -i input.m4a -ar 16000 -ac 1 -c:a pcm_s16le output.wav
  ```
- For very long content (>3 hours), split audio into 30-minute chunks, process in parallel, and merge results

---

## Environment Variables

| Variable | Description |
|---|---|
| `REDIS_URL` | Queue connection |
| `MINIO_ENDPOINT` | MinIO host |
| `MINIO_ACCESS_KEY` | MinIO credentials |
| `MINIO_SECRET_KEY` | MinIO credentials |
| `HF_TOKEN` | HuggingFace token for pyannote |
| `WHISPER_MODEL` | Model size: `tiny`, `base`, `medium`, `large-v2` (default: `large-v2`) |
| `DEVICE` | `cuda` or `cpu` |
| `COMPUTE_TYPE` | `float16` (GPU) or `int8` (CPU fallback) |

---

## Notes on Speaker Identity

The transcription service only produces opaque speaker IDs (`SPEAKER_00`, `SPEAKER_01`). Resolving these to actual person names is the responsibility of the **Profiling Service**, which can:

- Match known speaker voice embeddings (future capability)
- Use contextual clues (the host mentions a guest's name early in the recording)
- Allow manual correction via the API
