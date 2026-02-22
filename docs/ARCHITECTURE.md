# Architecture

## Overview

The platform is built as a pipeline of loosely coupled microservices. Each service reads from a queue, does one job well, and either writes results to object storage or publishes to the next queue stage.

---

## Data Flow Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                          INGESTION LAYER                            │
│                                                                     │
│  YouTube ──┐                                                        │
│  Twitter ──┤                                                        │
│  Substack ─┼──► Ingestion Service ──► MinIO (raw media)            │
│  PDF/Text ─┤         │                                              │
│  RSS ──────┘         │                                              │
│                      ▼                                              │
│              [queue: media.ingested]                                │
└─────────────────────────────────────────────────────────────────────┘
                       │
        ┌──────────────┼──────────────┐
        ▼              ▼              ▼
┌──────────────┐ ┌──────────────┐ ┌──────────────────────────────────┐
│TRANSCRIPTION │ │  OCR / FRAME │ │       TEXT EXTRACTION            │
│   SERVICE    │ │  EXTRACTION  │ │  (for PDFs, plain text, tweets)  │
│              │ │   SERVICE    │ │                                  │
│ WhisperX +   │ │ ffmpeg +     │ │ pdfplumber, html2text, etc.      │
│ pyannote     │ │ PaddleOCR    │ │                                  │
└──────┬───────┘ └──────┬───────┘ └──────────────┬───────────────────┘
       │                │                         │
       └────────────────┴─────────────────────────┘
                        │
                        ▼
               [queue: content.extracted]
                        │
                        ▼
┌─────────────────────────────────────────────────────────────────────┐
│                       ENRICHMENT SERVICE                            │
│                                                                     │
│  - Named Entity Recognition (people, orgs, locations, products)    │
│  - Keyword & keyphrase extraction                                   │
│  - Sentiment analysis (per segment + document-level)               │
│  - Language detection                                               │
│  - Topic modeling                                                   │
│  - Embedding generation (for semantic search)                       │
└──────────────────────────────┬──────────────────────────────────────┘
                               │
                               ▼
                  [queue: content.enriched]
                               │
              ┌────────────────┴────────────────┐
              ▼                                  ▼
┌─────────────────────────┐       ┌──────────────────────────────────┐
│    PROFILING SERVICE    │       │         INDEXER SERVICE          │
│                         │       │                                  │
│  Aggregates entities    │       │  Writes documents to             │
│  across all sources     │       │  Elasticsearch:                  │
│  into unified profiles  │       │  - media_segments index          │
│  for people, orgs,      │       │  - media_documents index         │
│  and channels           │       │  - entity_profiles index         │
└─────────────────────────┘       └──────────────────────────────────┘
```

---

## Services

### 1. Ingestion Service
**Responsibility**: Download or fetch raw media from any supported source and store it in object storage.

**Inputs**: A job request (source URL or identifier, source type)
**Outputs**: Raw media file(s) in MinIO + a `MediaObject` metadata document
**Publishes to**: `queue: media.ingested`

See: [`services/INGESTION.md`](../services/INGESTION.md)

---

### 1b. Web Parsing Service
**Responsibility**: Fetch and render websites, then extract normalized text, links, images, and audio URLs for downstream NLP and indexing.

**Inputs**: Web URL jobs (direct URLs, RSS discovered links, source adapters)
**Outputs**: `WebDocument` payload containing canonical URL, main text, link graph, image assets, audio assets
**Publishes to**: `queue: content.extracted`

See: [`services/WEB_PARSING.md`](../services/WEB_PARSING.md)

---

### 1c. Archiving Service
**Responsibility**: Archive source pages for reproducibility and legal/compliance traceability using a self-hosted ArchiveBox instance.

**Inputs**: URL + crawl metadata from ingestion/parsing stages
**Outputs**: Archived snapshots (WARC/HTML/assets) and archive IDs persisted to metadata storage
**Publishes to**: optional `queue: content.archived` notifications

See: [`services/ARCHIVING.md`](../services/ARCHIVING.md)

---

### 2. Transcription Service
**Responsibility**: Convert audio/video to time-stamped, speaker-labeled transcript segments.

**Inputs**: `MediaObject` with path to audio/video in MinIO
**Outputs**: Array of `TranscriptSegment` objects
**Publishes to**: `queue: content.extracted`

See: [`services/TRANSCRIPTION.md`](../services/TRANSCRIPTION.md)

---

### 3. OCR / Frame Extraction Service
**Responsibility**: Extract frames from video, detect scenes with significant text (slides, presentations, on-screen captions), and OCR them.

**Inputs**: `MediaObject` with path to video in MinIO
**Outputs**: Array of `OcrFrame` objects with extracted text and timestamps
**Publishes to**: `queue: content.extracted`

See: [`services/OCR.md`](../services/OCR.md)

---

### 4. Enrichment Service
**Responsibility**: Perform NLP analysis on all extracted text — transcripts, OCR, article bodies.

**Inputs**: Unified `ContentDocument` from the extraction stage
**Outputs**: `EnrichedDocument` with entities, keywords, sentiment, topics, embeddings
**Publishes to**: `queue: content.enriched`

See: [`services/ENRICHMENT.md`](../services/ENRICHMENT.md)

---

### 5. Profiling Service
**Responsibility**: Maintain and update unified profiles for named entities (people, organizations, channels) by aggregating appearances across all processed content.

**Inputs**: `EnrichedDocument` stream
**Outputs**: Upserted `EntityProfile` documents in Elasticsearch
**Publishes to**: nothing (terminal stage)

See: [`services/PROFILING.md`](../services/PROFILING.md)

---

### 6. Indexer Service
**Responsibility**: Write all processed content into Elasticsearch with the correct index mappings.

**Inputs**: `EnrichedDocument`
**Outputs**: Documents in Elasticsearch indices
**Publishes to**: nothing (terminal stage)

See: [`services/INDEXER.md`](../services/INDEXER.md)

---

## Queue Event Schema

All queue messages follow this envelope:

```json
{
  "event_id": "uuid-v4",
  "event_type": "media.ingested | content.extracted | content.enriched",
  "timestamp": "ISO-8601",
  "source_id": "unique ID of the original media object",
  "payload_location": "minio://bucket/path/to/payload.json",
  "metadata": {
    "source_type": "youtube | twitter | substack | pdf | text | rss | website",
    "retry_count": 0
  }
}
```

Payloads larger than ~64KB are stored in MinIO and referenced by `payload_location`. Small payloads may be inlined in the message body.

---

## Error Handling Strategy

- Each service wraps processing in try/except and publishes failures to a `queue: dead_letter` queue
- Dead letter messages include the original event + error traceback
- A monitoring service (future) will alert on DLQ depth and allow manual replay
- All services are idempotent: re-processing the same `source_id` overwrites previous results cleanly

---

## Scalability Notes

- Each service is stateless and can be horizontally scaled by adding more Celery workers
- GPU-heavy services (Transcription, Enrichment embeddings) should be scaled on GPU nodes
- CPU-bound services (OCR, keyword extraction) can run on standard compute
- The Profiling service must handle concurrent writes carefully — use Elasticsearch's `update` API with `retry_on_conflict`
