# Media Intelligence Platform

## Approved target direction (2026-10-14; migration not yet implemented)

Media Intelligence is to become an **embeddable Rust multimodal analysis and retrieval-composition library** shared by corpus applications. This replaces the legacy premise that the repository's primary product is a Python microservice pipeline.

- **Producer ownership stays below this project.** Invoke existing audio/transcription, visual/OCR, scene-detection and NLP libraries. Media Intelligence owns reusable planning/composition of those capabilities, evidence alignment across modalities, and cross-modal retrieval/result assembly; it does not recreate their algorithms, model runtimes or generic text-ranking/indexing implementations.
- **Option A: corpus-owned sources and indexes.** The consuming corpus owns its source identity, authoritative metadata, original assets, index-lifecycle decisions, query UX and access policy; an optional independent `search-engine` provides reusable index/query mechanisms without taking ownership of source data. Media Intelligence provides typed reusable analysis/result contracts and corpus-neutral cross-modal fusion against caller-supplied evidence/index adapters. No mandatory central index, copied corpus, or Elasticsearch/Qdrant service.
- **Consumer boundaries.** YouTube Corpus owns YouTube discovery, channel/playlist monitoring, downloads and metadata; Document Search owns local document ingestion, source paths, offline/browser persistence and UX; Media Similarity owns local-folder/S3 source configuration, quality evaluation, its Qdrant and Pages index variants, similarity-product behavior and UI. These products may adopt common Media Intelligence analysis/composition without losing their existing standalone modes.
- **Evidence correctness.** Preserve source/revision identity, modality, timestamp or page/span locators, detector/model/configuration provenance, unknown/unprocessed status and independent score semantics. Multimodal alignment is not person-belief attribution or verification of a proposition's truth; downstream interpretation remains with its owning products.
- **Migration approach.** Extract/adapt demonstrated common behavior from existing Rust consumers with compatibility and independent acceptance before removing old implementations. Keep existing consumer behavior working; do not start a flag-day rewrite. Rust is the target implementation language, with optional transport/bindings only when real consumers require them; no obligatory Python/Redis/MinIO/Elasticsearch runtime for the core.

**Subsequent owner decision (2026-10-14):** establish a separately owned, domain-neutral Rust `search-engine` repository now, independent of `nlp-stack` and Media Intelligence, for typed structured metadata, Boolean/range filters, facets, generic lexical search and rebuildable search indexes ([bootstrap issue #35](https://github.com/moritzbrantner/media-intelligence/issues/35)). Corpus products still own canonical source records, original assets, permissions, indexing lifecycle and user-facing search policy. Media Intelligence continues to own cross-modal evidence alignment and result fusion, not generic indexing. Existing `nlp-stack` text search remains valid until any objectively shared generic portion can migrate with demonstrated consumer parity. A centralized/federated/global index or compulsory common database is **not** approved by this decision.

### Rust core: `crates/media-intelligence-core`

The first slice of the target library composes caller-supplied, already-recorded speech (Foundation `TimedTextContract` or plain segments), scene and OCR producer runs for one caller-owned media object into one deterministic, provenance-bearing timeline (`compose`). It reports per-modality coverage (not requested, unprocessed, failed, completed-empty, completed-with-findings), keeps every producer receipt and modality-specific confidence, collapses repeated OCR frames only through producer-supplied tracks, and derives scene/speech/visible-text overlap relations. It needs no Python, database, network or model runtime; `compat::youtube_corpus` accepts YouTube Corpus `MediaEvidenceBatchV1` exports.

```bash
cargo test --locked -p media-intelligence-core
cargo run --locked -p media-intelligence-core --example compose -- crates/media-intelligence-core/tests/fixtures/lecture_composition.json
```

### Producer execution: `crates/media-intelligence-analysis`

`analyze` runs one caller-owned local media file through a selected profile: scene detection executes for real through the canonical `visual-analysis` path (`FfmpegVideoSource` + `detect_content_scenes`, exact-rev pinned; needs `ffmpeg`/`ffprobe` on `PATH`), while speech and visible text are typed producer seams the caller fills (a requested modality without a producer is reported **unavailable**). The content hash is verified before any stage, results are reused through a caller-owned stage cache keyed by content hash + producer identity + configuration, cancellation stops new stages, and a failed stage never discards the others. With the opt-in `audio-analysis` feature, `speech::AudioAnalysisSpeechProducer` runs speech recognition through `audio-analysis` transcription (exact-rev pinned; the caller picks the provider and supplies local models, and download-capable configurations are refused unless `ModelFetchPolicy::AllowNetwork` is set; `audio-analysis-candle` enables the native Candle Whisper runtime). With the opt-in `visual-analysis-ocr` feature, `visible_text::VisualAnalysisOcrProducer` runs OCR through visual-analysis `analyze_scene_aware_ocr` on the scene intervals of the same analysis (stages run speech → scenes → visible text) with a caller-selected backend (local ONNX TrOCR via `visual-analysis-ocr-onnx`, or a JSON external command); repeated frames collapse through visual-analysis' own text tracks. Model-backed acceptance on a real video is #34.

**Owner-approved OCR provider choice (2026-10-15, [#34](https://github.com/moritzbrantner/media-intelligence/issues/34), option B):** For the real-video slide-text acceptance, use an **established local OCR engine with text detection and recognition** through the existing `visual-analysis` JSON external-command backend (`OcrBackendConfig::JsonCommand`); try **RapidOCR** (PaddleOCR detection + recognition via ONNX) first, with **Tesseract** as an alternative if measured real-video results warrant it. The whole-frame `trocr-base-printed` run in draft [PR #41](https://github.com/moritzbrantner/media-intelligence/pull/41) produced unusable slide text and **does not pass OCR acceptance**. Retain TrOCR as an available backend, but neither add custom line detection to visual-analysis nor weaken the independent slide-text assertion to accommodate that failure. Any adapter must emit the existing `OcrDocument` JSON contract. Keep OCR dependencies and models explicitly provisioned, local and outside the repository; record the chosen engine/model versions, inputs, configuration, hashes, licensing and evidence receipts, and retain stage-cache, failure and cancellation semantics. This is approved implementation direction, **not** a claim that RapidOCR or Tesseract has passed the real-video test. #34 / PR #41 own the remaining implementation and acceptance evidence.
Running real producers on a video is [#27](https://github.com/moritzbrantner/media-intelligence/issues/27); retrieval fusion over corpus-owned indexes is [#26](https://github.com/moritzbrantner/media-intelligence/issues/26). The Python transcription, OCR, enrichment and search services stay as compatibility-only legacy surfaces; they are retired under [#16](https://github.com/moritzbrantner/media-intelligence/issues/16) only after YouTube Corpus, Document Search and Media Similarity each pass independent acceptance on the Rust core.

**Status:** the Python services, contracts and operational instructions below describe existing/legacy implementation surfaces, not proof that this target architecture is implemented. The recorded bounded-context split in [ADR 0001](docs/decisions/0001-bounded-context-monorepo.md) remains historical context for those services; its Python/microservice structure is not the new design authority. Implementation work and acceptance are tracked in GitHub issues.

---


A modular, microservice-based platform for ingesting, transcribing, analyzing, and indexing media from multiple sources — YouTube, X/Twitter, Substack, PDFs, plain text, and more — with the goal of building rich, searchable profiles of people, organizations, and channels.

### Downstream Rust source consumption

Use an immutable public Git revision of `media-intelligence-analysis` for source
integration. Enabling `visual-analysis-ocr` currently also needs the unpublished
Foundation semantic primitive. Cargo does not inherit a dependency workspace's
`[patch.crates-io]`, so the consuming Cargo root must declare this one override:

```toml
[dependencies]
media-intelligence-analysis = { git = "https://github.com/moritzbrantner/media-intelligence", rev = "c9206895de086b9716c3232edeeb9fc10306600f", features = ["visual-analysis-ocr"] }

[patch.crates-io]
moenarch-semantic-core = { git = "https://github.com/moritzbrantner/moenarch-foundation", rev = "b0e26e4e671e5c43d659a2c67583ffdf5a9bbfa9" }
```

The other workspace patches are not part of the downstream requirement. This is
an exact source contract, not a claim that these packages are published or that
model-backed OCR acceptance has run. No sibling checkout, credentials, model
download or registry publication is needed to compile the OCR facade.

Run `bash scripts/check-downstream-ocr-consumer.sh` to verify a clean Git consumer
of the current commit with this minimal override. It creates a disposable crate,
fetches dependencies explicitly, then compiles the real `analyze` and OCR adapter
exports offline with a lockfile. CI runs this check in addition to workspace
checks, which alone cannot prove downstream patch behavior.

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
