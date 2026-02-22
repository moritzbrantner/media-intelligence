# Media Intelligence Platform

A modular, microservice-based platform for ingesting, transcribing, analyzing, and indexing media from multiple sources — YouTube, X/Twitter, Substack, PDFs, plain text, and more — with the goal of building rich, searchable profiles of people, organizations, and channels.

## What It Does

- **Ingest** media from YouTube, X/Twitter, Substack, RSS, PDFs, and plain text
- **Transcribe** audio and video with speaker diarization (who said what, when)
- **OCR** video frames and documents to capture on-screen text, slides, and presentations
- **Enrich** content with NLP: named entities, keywords, topics, sentiment, language detection
- **Profile** people, organizations, and channels across sources
- **Index** everything into Elasticsearch for powerful full-text and semantic search


## Lightweight Development Mode

To keep local setup fast, `docker-compose.yml` now starts a single `mock-service` by default.
All heavyweight platform services remain in the compose file but are intentionally commented out.

- Start with Docker (optional): `docker compose up mock-service`
- Start bare-metal (no Docker): `python scripts/mock_service.py`
- Health check: `curl http://localhost:${MOCK_SERVICE_PORT:-8080}/health`
- Example mock data: `curl http://localhost:${MOCK_SERVICE_PORT:-8080}/v1/jobs?limit=5`

If Docker is unavailable in your Codex runtime, that is an environment capability limitation (not a repo setting). In that case, use the bare-metal command above.

When you want to work on a real service, create or switch to its dedicated branch and enable only that service plus dependencies in `docker-compose.yml`.

---

## Repository Structure

```
media-intelligence/
├── README.md                        ← You are here
├── docs/
│   ├── ARCHITECTURE.md              ← System design & data flow
│   ├── DATA_MODELS.md               ← Elasticsearch schemas & entity models
│   ├── CONTRIBUTING.md              ← How to contribute & agent instructions
│   ├── ROADMAP.md                   ← Phased feature roadmap
│   └── IMPLEMENTATION_TASKS.md      ← Execution-ready engineering backlog
├── services/
│   ├── INGESTION.md                 ← Ingestion service spec
│   ├── TRANSCRIPTION.md             ← Transcription & diarization service spec
│   ├── OCR.md                       ← Frame extraction & OCR service spec
│   ├── ENRICHMENT.md                ← NLP enrichment service spec
│   ├── PROFILING.md                 ← Entity profiling service spec
│   └── INDEXER.md                   ← Elasticsearch indexing service spec
└── infrastructure/
    └── INFRASTRUCTURE.md            ← Storage, queues, deployment
```

---

## Quick Start (for Agents)

1. Read `docs/ARCHITECTURE.md` to understand the overall data flow
2. Read `docs/DATA_MODELS.md` to understand the shared data contracts between services
3. Pick a service from `services/` to implement
4. Follow conventions in `docs/CONTRIBUTING.md`

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
| Storage | MinIO (S3-compatible) |
| Queue | Redis + Celery |
| Transcription | WhisperX, `pyannote.audio` |
| OCR | PaddleOCR, `ffmpeg`, `PySceneDetect` |
| NLP | spaCy, HuggingFace Transformers, BERTopic, KeyBERT |
| Profiling | Custom aggregation service |
| Search | Elasticsearch 8.x |
| Orchestration | Docker Compose (dev), Kubernetes (prod) |
| Language | Python 3.11+ |
