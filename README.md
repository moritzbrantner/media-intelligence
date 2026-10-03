# Media Intelligence Platform

A modular, microservice-based platform for ingesting, transcribing, analyzing, and indexing media from multiple sources — YouTube, X/Twitter, Substack, PDFs, plain text, and more — with the goal of building rich, searchable profiles of people, organizations, and channels.

## What It Does

- **Ingest** media from YouTube, X/Twitter, Substack, RSS, PDFs, and plain text
- **Transcribe** audio and video with speaker diarization (who said what, when)
- **OCR** video frames and documents to capture on-screen text, slides, and presentations
- **Enrich** content with NLP: named entities, keywords, topics, sentiment, language detection
- **Profile** people, organizations, and channels across sources
- **Index** everything into Elasticsearch for powerful full-text and semantic search


## Development Layout

The repository is staged into bounded-context subprojects:

- `contracts/` contains the versioned shared schemas in `mi_contracts`
- `projects/discovery/` contains `mi_discovery`
- `projects/extraction/` contains `mi_extraction`
- `projects/understanding/` contains `mi_understanding`
- `projects/search/` contains `mi_search`
- `ops/` contains compose files, shared env templates, and operational scripts
- `services/` remains as a temporary compatibility layer for legacy imports and entrypoints

## Lightweight Development Mode

Use the split compose files under `ops/compose/` for bounded-context work:

- Discovery: `docker compose -f ops/compose/discovery.yml up`
- Extraction: `docker compose -f ops/compose/extraction.yml up`
- Understanding: `docker compose -f ops/compose/understanding.yml up`
- Search: `docker compose -f ops/compose/search.yml up`
- Full stack: `docker compose -f ops/compose/full-stack.yml up`

Compatibility entrypoints still work from the repo root:

- `python services/ingestion/main.py`
- `python services/ocr/main.py`
- `python services/enrichment/main.py`
- `python -m services.source_watcher.main`
- `python -m services.search.main`

---

## Repository Structure

```text
media-intelligence/
├── README.md
├── contracts/
│   ├── src/mi_contracts/            ← Shared versioned schemas
│   ├── schemas/v1/                  ← Generated JSON Schemas
│   └── tests/
├── docs/
│   ├── ARCHITECTURE.md
│   ├── DATA_MODELS.md
│   ├── CONTRIBUTING.md
│   ├── ROADMAP.md
│   ├── IMPLEMENTATION_TASKS.md
│   └── decisions/
├── ops/
│   ├── compose/
│   ├── env/
│   └── scripts/
├── projects/
│   ├── discovery/
│   ├── extraction/
│   ├── search/
│   └── understanding/
├── services/                        ← Temporary compatibility layer
└── infrastructure/
    └── INFRASTRUCTURE.md
```

---

## Development Utilities

Install the shared lightweight dev dependency used by the documentation validation scripts:

```bash
pip install -r requirements-dev.txt
```

Run YAML validation used by the inference architecture artifacts:

```bash
./scripts/validate-yaml.sh
```

If `pip install` fails with proxy/network errors (for example `403 Forbidden`), either:

- use an approved internal package mirror (`PIP_INDEX_URL`) with reachable proxy settings, or
- run the fallback parser path via Ruby (already supported by `./scripts/validate-yaml.sh`).

## Quick Start

1. Read `docs/ARCHITECTURE.md` to understand the overall data flow.
2. Read `contracts/src/mi_contracts/` and `docs/DATA_MODELS.md` for the shared contracts.
3. Work inside the bounded-context project that owns the code you are changing.
4. Run tests with `python -m pytest` from that project root.
5. Use `services/` only for compatibility during migration.

---

## Core Design Principles

- **Contract-first**: Every service communicates via well-defined JSON schemas. Data models are the source of truth.
- **Idempotent**: Any service can be re-run on the same input without side effects.
- **Decoupled**: Services communicate via a message queue. No direct service-to-service calls.
- **Extensible**: Adding a new media source means adding a new ingestion adapter, not modifying existing services.
- **Observable**: Every service emits structured logs and metrics.

---

## Tech Stack Summary

| Layer | Technology |
|---|---|
| Ingestion | Python, `yt-dlp`, `tweepy`, `requests` |
| Web Parsing | Playwright + Trafilatura + BeautifulSoup/lxml |
| Archiving | ArchiveBox (self-hosted) |
| Storage | MinIO (S3-compatible) |
| Queue | Redis + Celery |
| Transcription | WhisperX, `pyannote.audio` |
| OCR | PaddleOCR, `ffmpeg`, `PySceneDetect` |
| NLP | spaCy, HuggingFace Transformers, BERTopic, KeyBERT |
| Profiling | Custom aggregation service |
| Search | Elasticsearch 8.x |
| Orchestration | Docker Compose (dev), Kubernetes (prod) |
| Language | Python 3.11+ |
