# Contributing Guide

This document is written for both human developers and AI agents working on this project. Please read it fully before writing any code.

---

## For AI Agents: How to Work on This Project

If you are an AI agent assigned to implement part of this system, follow this checklist before writing any code:

1. **Read `docs/ARCHITECTURE.md`** — understand where your service fits in the pipeline
2. **Read `docs/DATA_MODELS.md`** — this is the contract. Your inputs and outputs must match these schemas exactly
3. **Read the service spec file** in `services/` that corresponds to your assignment
4. **Read this document** (you're doing that now)
5. **Do not modify** `docs/DATA_MODELS.md` or `docs/ARCHITECTURE.md` without explicit instruction — these are shared contracts
6. **Do not introduce new dependencies** without adding them to the relevant service's `requirements.txt` and noting them in the service spec

---

## Project Structure

Each service lives in its own directory:

```
services/
├── ingestion/
│   ├── requirements.txt
│   ├── main.py             ← Celery worker entrypoint
│   ├── adapters/           ← One file per source type (youtube.py, twitter.py, ...)
│   └── tests/
├── transcription/
│   ├── requirements.txt
│   ├── main.py
│   └── tests/
├── ocr/
├── enrichment/
├── profiling/
└── indexer/
```

When scaffolding a new service, create this directory structure.

---

## Code Standards

### Language & Runtime
- Python 3.11+
- Type hints everywhere (use `from __future__ import annotations`)
- Dataclasses or Pydantic models for all data structures (prefer Pydantic v2)

### Style
- `black` for formatting (line length 100)
- `ruff` for linting
- Docstrings on all public functions and classes

### Patterns

**All services must follow this worker pattern:**

```python
from celery import Celery
from pydantic import BaseModel
import logging

logger = logging.getLogger(__name__)
app = Celery(...)

@app.task(bind=True, max_retries=3, default_retry_delay=60)
def process(self, event: dict) -> None:
    try:
        # 1. Deserialize input
        # 2. Fetch payload from MinIO if needed
        # 3. Do the work
        # 4. Write output to MinIO
        # 5. Publish next event to queue
        pass
    except Exception as exc:
        logger.exception("Task failed: %s", exc)
        raise self.retry(exc=exc)
```

**All output schemas must be validated with Pydantic before publishing.**

---

## Queue Conventions

- Queue names use dot notation: `media.ingested`, `content.extracted`, `content.enriched`, `dead.letter`
- Always include `event_id` (uuid4), `event_type`, `timestamp`, and `source_id` in every message
- Large payloads (>64KB) must be stored in MinIO and referenced by path — never inline them in the queue message

---

## MinIO Storage Layout

```
bucket: raw-media/
  {source_type}/{yyyy-mm-dd}/{media_id}/
    original.{ext}          ← raw downloaded file
    metadata.json           ← MediaObject JSON

bucket: processed/
  {media_id}/
    transcript.json         ← [TranscriptSegment]
    ocr_frames.json         ← [OcrFrame]
    enriched.json           ← EnrichedDocument

bucket: frames/
  {media_id}/
    frame_{index:06d}.jpg   ← extracted video frames (scene changes only)
```

---

## Testing Requirements

Every service must include:

- Unit tests for each adapter/component
- An integration test that runs the full worker with a sample payload
- Test fixtures stored in `tests/fixtures/`

Run tests with: `pytest services/{service_name}/tests/`

---

## Adding a New Media Source

To add a new source type (e.g., a new podcast platform):

1. Add a new adapter in `services/ingestion/adapters/{source_name}.py`
2. Implement the `BaseIngestionAdapter` interface (see `services/INGESTION.md`)
3. Register the adapter in `services/ingestion/main.py`
4. Add the new `source_type` string to the `MediaObject` schema documentation in `docs/DATA_MODELS.md`
5. Add a test fixture and integration test
6. Update `docs/ROADMAP.md` to mark the source as implemented

---

## Git Conventions

- Branch names: `feature/{service}-{short-description}` or `fix/{service}-{short-description}`
- Commit messages: `[service] Short description of what changed`
- Every PR must reference the relevant spec document and data model version
- Do not merge PRs that break the data model contracts without a coordinated version bump

---

## Environment Variables

All services read configuration from environment variables. Required variables per service are listed in that service's spec file. Common variables:

| Variable | Description |
|---|---|
| `REDIS_URL` | Redis connection string |
| `MINIO_ENDPOINT` | MinIO endpoint (e.g. `localhost:9000`) |
| `MINIO_ACCESS_KEY` | MinIO access key |
| `MINIO_SECRET_KEY` | MinIO secret key |
| `ELASTICSEARCH_URL` | Elasticsearch URL |
| `LOG_LEVEL` | `DEBUG`, `INFO`, `WARNING` |
