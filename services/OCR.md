# Service Spec: OCR / Frame Extraction

## Responsibility

Extract frames from video at scene changes, detect frames with significant text content (slides, presentations, captions, on-screen graphics), run OCR on those frames, and store the extracted text with timestamps.

Also handles OCR for PDF page images produced by the Ingestion service.

---

## Interface

### Input

Consumes from `queue: media.ingested`.

Expects a `MediaObject` where at least one `media_file` has `type: video` or `type: image`.

For PDFs, processes page images already stored in MinIO by the Ingestion service.

### Output

A list of `OcrFrame` objects stored at `minio://processed/{media_id}/ocr_frames.json`.

### Publishes To

`queue: content.extracted`

---

## Processing Steps

### For Video

1. **Scene change detection** — use PySceneDetect to find meaningful cut points (avoids processing thousands of near-identical frames)
2. **Supplemental low-FPS extraction** — also extract 1 frame every 5 seconds to catch slow transitions (e.g. slides that fade in)
3. **Deduplication** — hash frames, skip near-duplicates (perceptual hash with threshold)
4. **Text region detection** — pre-filter frames using a fast heuristic: does this frame contain large blocks of text? (use OpenCV edge detection or a lightweight classifier)
5. **OCR** — run PaddleOCR on frames that pass the text detection filter
6. **Post-process** — clean OCR output (strip junk characters, normalize whitespace, filter very short results)
7. **Store thumbnails** — save filtered frames as JPEG to `minio://frames/{media_id}/frame_{index:06d}.jpg`
8. **Assemble and store** `ocr_frames.json`

### For PDFs

1. Fetch page images from MinIO (stored by Ingestion as `{media_id}/page_{n}.png`)
2. Run PaddleOCR on each page
3. Store results as `OcrFrame` objects (use `frame_index = page_number`, `timestamp_seconds = null`)

---

## Technology

### PySceneDetect — Scene Change Detection

```python
from scenedetect import detect, ContentDetector

scene_list = detect(video_path, ContentDetector(threshold=27.0))
for scene in scene_list:
    start_time = scene[0].get_seconds()
    # extract frame at start_time with ffmpeg
```

### ffmpeg — Frame Extraction

```bash
# Extract frame at specific timestamp
ffmpeg -ss {timestamp} -i {video_path} -frames:v 1 -q:v 2 {output_path}

# Extract 1fps (supplemental)
ffmpeg -i {video_path} -vf fps=1/5 frame_%06d.jpg
```

### PaddleOCR

```python
from paddleocr import PaddleOCR

ocr = PaddleOCR(use_angle_cls=True, lang='en', show_log=False)

result = ocr.ocr(image_path, cls=True)
text_blocks = []
for line in result[0]:
    bbox, (text, confidence) = line
    text_blocks.append({
        "text": text,
        "confidence": confidence,
        "bounding_box": {
            "x": int(bbox[0][0]),
            "y": int(bbox[0][1]),
            "width": int(bbox[2][0] - bbox[0][0]),
            "height": int(bbox[2][1] - bbox[0][1])
        }
    })
```

### Text Region Pre-Filter (OpenCV heuristic)

Before running the more expensive PaddleOCR on every frame, filter using this heuristic:

```python
import cv2
import numpy as np

def has_significant_text(image_path: str, threshold: float = 0.02) -> bool:
    """
    Returns True if the image likely contains significant text content.
    Uses edge density as a proxy for text presence.
    """
    img = cv2.imread(image_path, cv2.IMREAD_GRAYSCALE)
    edges = cv2.Canny(img, 100, 200)
    edge_density = np.sum(edges > 0) / edges.size
    return edge_density > threshold
```

This is a fast pre-filter. Tune the threshold based on your content type.

---

## Slide Detection

A frame is classified as a "slide" (presentation content) if it meets these criteria:

- Large uniform background region (white, light grey, or brand color)
- Text occupying >10% of frame area
- Aspect ratio consistent with 4:3 or 16:9 slides
- Frame appears for >3 seconds (not a quick cutaway)

Slides get `scene_change: true` and are prioritized for full OCR regardless of the pre-filter result.

---

## Output Format

```json
[
  {
    "media_id": "youtube_abc123def456",
    "frame_index": 42,
    "timestamp_seconds": 134.5,
    "scene_change": true,
    "text_blocks": [
      {
        "text": "Bitcoin: A Peer-to-Peer Electronic Cash System",
        "confidence": 0.98,
        "bounding_box": { "x": 120, "y": 80, "width": 1040, "height": 60 }
      },
      {
        "text": "Satoshi Nakamoto",
        "confidence": 0.95,
        "bounding_box": { "x": 400, "y": 200, "width": 400, "height": 40 }
      }
    ],
    "full_text": "Bitcoin: A Peer-to-Peer Electronic Cash System\nSatoshi Nakamoto",
    "storage_path": "minio://frames/youtube_abc123def456/frame_000042.jpg"
  }
]
```

---

## Performance Notes

- OCR is CPU-bound; PaddleOCR with GPU support is ~5x faster
- For a typical 1-hour video, expect 50–200 meaningful frames after filtering
- Pre-filtering with OpenCV before PaddleOCR reduces processing time by ~80% for content-heavy videos
- Process frames in parallel batches (use `multiprocessing.Pool` or async tasks)

---

## Environment Variables

| Variable | Description |
|---|---|
| `REDIS_URL` | Queue connection |
| `MINIO_ENDPOINT` | MinIO host |
| `MINIO_ACCESS_KEY` | MinIO credentials |
| `MINIO_SECRET_KEY` | MinIO credentials |
| `MINIO_FRAMES_BUCKET` | Bucket for frame thumbnails (default: `frames`) |
| `OCR_LANG` | PaddleOCR language (default: `en`, supports multilingual) |
| `SCENE_THRESHOLD` | ContentDetector threshold (default: `27.0`) |
| `FRAME_SAMPLE_RATE` | Supplemental extraction rate in seconds (default: `5`) |
| `TEXT_DENSITY_THRESHOLD` | OpenCV pre-filter threshold (default: `0.02`) |
| `USE_GPU` | Enable GPU for PaddleOCR (default: `false`) |
