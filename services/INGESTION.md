# Service Spec: Ingestion

## Responsibility

Download or fetch raw media from a supported source, store it in MinIO, and publish a `media.ingested` event so downstream services can begin processing.

---

## Supported Source Types (Phase 1+)

| Source | Adapter | Status |
|---|---|---|
| YouTube (video/audio) | `yt-dlp` | Phase 1 |
| X/Twitter (tweets, threads) | `tweepy` / snscrape | Phase 3 |
| Substack (articles) | RSS + HTML scraper | Phase 3 |
| PDF | file upload / URL fetch | Phase 3 |
| Plain text | direct input | Phase 3 |
| RSS/Atom feeds | `feedparser` | Phase 3 |
| Podcast RSS (audio) | `feedparser` + `requests` | Phase 3 |

---

## Interface

### Input: Ingestion Job Request

Submitted via the API or CLI. Published to `queue: ingestion.jobs`.

```json
{
  "job_id": "uuid-v4",
  "source_type": "youtube",
  "source_url": "https://www.youtube.com/watch?v=...",
  "options": {
    "download_video": true,
    "download_audio_only": false,
    "max_resolution": "1080p"
  }
}
```

### Output: `MediaObject`

See `docs/DATA_MODELS.md` for the full schema. The ingestion service is responsible for populating all fields it can from the source metadata.

### Publishes To

`queue: media.ingested` — see Architecture doc for event envelope format.

---

## Adapter Interface

All source adapters must implement this base class:

```python
from abc import ABC, abstractmethod
from models import MediaObject, IngestionJob

class BaseIngestionAdapter(ABC):

    @abstractmethod
    def fetch(self, job: IngestionJob) -> MediaObject:
        """
        Download or fetch media from the source.
        Must store all files in MinIO and return a fully populated MediaObject.
        """
        ...

    @abstractmethod
    def supports(self, source_type: str) -> bool:
        """Return True if this adapter handles the given source_type."""
        ...
```

---

## YouTube Adapter

**Library**: `yt-dlp`

**Steps**:
1. Call `yt-dlp` with the video URL to extract metadata (no download yet)
2. Parse title, description, uploader, channel, upload date, tags, duration, language
3. Download the best audio stream (for transcription) — store as `{media_id}/audio.m4a`
4. Optionally download video — store as `{media_id}/video.mp4`
5. Store raw `yt-dlp` info dict as `{media_id}/metadata.json`
6. Return populated `MediaObject`

**Key yt-dlp options**:
```python
ydl_opts = {
    'format': 'bestaudio/best',
    'outtmpl': '/tmp/%(id)s.%(ext)s',
    'writeinfojson': True,
    'quiet': True,
    'no_warnings': True,
}
```

**Metadata mapping**:

| yt-dlp field | MediaObject field |
|---|---|
| `title` | `title` |
| `description` | `description` |
| `uploader` | `author.name` |
| `uploader_id` | `author.id` |
| `uploader_url` | `author.profile_url` |
| `channel` | `channel.name` |
| `channel_id` | `channel.id` |
| `channel_url` | `channel.url` |
| `upload_date` | `published_at` (parse YYYYMMDD) |
| `duration` | `duration_seconds` |
| `language` | `language` |
| `tags` | `tags` |

---

## X/Twitter Adapter

**Library**: `tweepy` (API v2)

**Steps**:
1. Fetch tweet or thread by ID or URL
2. Expand full thread if it's a reply chain
3. Fetch author profile data (name, handle, bio, follower count)
4. Store tweet JSON in MinIO as a document file
5. For Twitter Spaces (audio): attempt to download with `yt-dlp` and store audio

**Note**: Twitter API rate limits are strict. Implement exponential backoff and cache results aggressively.

---

## Substack Adapter

**Library**: `feedparser`, `requests`, `beautifulsoup4`

**Steps**:
1. If given a newsletter URL, discover the RSS feed at `{url}/feed`
2. Parse feed entries to get article URLs and basic metadata
3. Fetch full article HTML and extract body text (strip nav, ads, etc.)
4. Store article HTML and extracted text in MinIO
5. For paid content: note `paywall: true` in `raw_metadata` and store partial text only

---

## PDF Adapter

**Library**: `pdfplumber`, `pymupdf`

**Steps**:
1. Download or receive PDF file
2. Extract text page by page with `pdfplumber`
3. Extract page images (for OCR service to process) with `pymupdf`
4. Store PDF, extracted text JSON, and page images in MinIO

---

## Environment Variables

| Variable | Description |
|---|---|
| `REDIS_URL` | Queue connection |
| `MINIO_ENDPOINT` | MinIO host |
| `MINIO_ACCESS_KEY` | MinIO credentials |
| `MINIO_SECRET_KEY` | MinIO credentials |
| `MINIO_RAW_BUCKET` | Bucket for raw media (default: `raw-media`) |
| `TWITTER_BEARER_TOKEN` | Twitter API v2 bearer token |
| `LOG_LEVEL` | Logging level |

---

## Error Handling

| Error | Behavior |
|---|---|
| Network timeout | Retry up to 3 times with exponential backoff |
| Video unavailable / private | Log warning, publish to dead letter with reason `UNAVAILABLE` |
| Rate limit hit | Retry after rate limit reset time (respect `Retry-After` header) |
| Storage failure | Retry, then dead letter |
| Unsupported source type | Immediately dead letter with reason `UNSUPPORTED_SOURCE` |
