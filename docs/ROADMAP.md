# Roadmap

This document tracks the phased build-out of the platform. Agents and contributors should use this to understand what is built, what is in progress, and what comes next.

Status key: `[ ]` not started · `[~]` in progress · `[x]` complete

---

## Phase 1 — Core Pipeline (YouTube → Elasticsearch)

The goal of Phase 1 is a working end-to-end pipeline for a single source type (YouTube) with the most critical analysis stages.

- [ ] **Ingestion: YouTube adapter** (`yt-dlp` download, metadata extraction)
- [ ] **MinIO storage setup** (raw-media and processed buckets)
- [ ] **Redis + Celery queue setup**
- [ ] **Transcription service** (WhisperX, speaker diarization via pyannote)
- [ ] **Enrichment service: NER** (spaCy, people and org extraction)
- [ ] **Enrichment service: keyword extraction** (KeyBERT)
- [ ] **Enrichment service: sentiment** (HuggingFace cardiffnlp model)
- [ ] **Indexer service** (write to `media_documents` and `media_segments` indices)
- [ ] **Elasticsearch index setup** (mappings for media_documents, media_segments)
- [ ] **Basic Kibana dashboard** (verify data is flowing)

---

## Phase 2 — OCR & Visual Analysis

- [ ] **Frame extraction service** (ffmpeg, 1fps + scene-change detection via PySceneDetect)
- [ ] **OCR service** (PaddleOCR on extracted frames)
- [ ] **Slide/presentation detection** (heuristic: large text regions, uniform backgrounds)
- [ ] **OCR text indexed** in `media_segments` with `segment_type: ocr`
- [ ] **Frame thumbnail storage** in MinIO `frames/` bucket

---

## Phase 3 — Additional Source Types

- [ ] **Ingestion: X/Twitter adapter** (tweets, threads, spaces audio via tweepy / snscrape)
- [ ] **Ingestion: Substack adapter** (article body via RSS + scraper)
- [ ] **Ingestion: PDF adapter** (pdfplumber text extraction + page images for OCR)
- [ ] **Ingestion: Plain text adapter** (direct text input)
- [ ] **Ingestion: RSS/Atom adapter** (generic feed reader)
- [ ] **Ingestion: Podcast RSS adapter** (audio download from podcast feeds)

---

## Phase 4 — Entity Profiling

- [ ] **Profiling service: person profiles** (aggregate appearances, mentions, topics)
- [ ] **Profiling service: organization profiles**
- [ ] **Profiling service: channel profiles** (YouTube channels, Substack newsletters, Twitter accounts)
- [ ] **Entity co-occurrence graph** (who appears alongside whom)
- [ ] **`entity_profiles` Elasticsearch index setup**
- [ ] **Entity resolution / deduplication** (merge "Elon Musk" and "@elonmusk" into one profile)
- [ ] **Speaker name resolution** (link diarized `SPEAKER_00` to known person entities)

---

## Phase 5 — Semantic Search & Topic Intelligence

- [ ] **Embedding generation** (sentence-transformers `all-MiniLM-L6-v2` or similar)
- [ ] **Semantic search endpoint** (kNN search via `dense_vector` in Elasticsearch)
- [ ] **BERTopic topic modeling** (run on full corpus, update periodically)
- [ ] **Topic trend analysis** (which topics are rising/falling over time per channel/person)
- [ ] **Cross-source correlation** (the same story told across YouTube, Twitter, Substack)

---

## Phase 6 — API & Interface

- [ ] **REST API** (FastAPI) for submitting ingestion jobs and querying profiles
- [ ] **Search API** (proxy to Elasticsearch with auth)
- [ ] **Webhook support** (notify external systems when a profile is updated)
- [ ] **Simple web UI** for search and profile browsing
- [ ] **Kibana dashboards** for entity frequency, sentiment trends, topic maps

---

## Phase 7 — Production Hardening

- [ ] **Kubernetes deployment manifests** (Helm chart)
- [ ] **GPU node configuration** for Transcription + Enrichment
- [ ] **Retry + dead letter queue monitoring**
- [ ] **Rate limiting** for external API calls (Twitter, etc.)
- [ ] **Deduplication** (skip re-processing already-seen `source_url`)
- [ ] **Incremental updates** (re-process a source on a schedule to catch new content)
- [ ] **Storage lifecycle policies** (archive raw media after N days)
- [ ] **Secrets management** (Vault or Kubernetes secrets)

---

## Out of Scope (for now)

- Real-time streaming (all processing is batch/async)
- Video generation or synthesis
- User authentication / multi-tenancy
- Legal compliance tooling (GDPR, DMCA) — handle separately
