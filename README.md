# Media Intelligence Platform

A modular, microservice-based platform for ingesting, transcribing, analyzing, and indexing media from multiple sources — YouTube, X/Twitter, Substack, PDFs, plain text, and more — with the goal of building rich, searchable profiles of people, organizations, and channels.

## What It Does

- **Ingest** media from YouTube, X/Twitter, Substack, RSS, PDFs, and plain text
- **Transcribe** audio and video with speaker diarization (who said what, when)
- **OCR** video frames and documents to capture on-screen text, slides, and presentations
- **Enrich** content with NLP: named entities, keywords, topics, sentiment, language detection
- **Profile** people, organizations, and channels across sources
- **Index** everything into Elasticsearch for powerful full-text and semantic search

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
