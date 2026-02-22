# Implementation Tasks

This task backlog translates the architecture and service specifications into an execution-ready plan.

## How to use this backlog

- Create one ticket per task ID.
- Keep task scope small enough to merge independently.
- Require passing tests/checks for each task before moving it to Done.
- Preserve contract compatibility with `docs/DATA_MODELS.md`.

---

## Epic 0 — Project Bootstrap & Developer Experience

### T-001: Scaffold repository service directories
**Goal:** Create baseline folder layout for each microservice.

**Deliverables**
- `services/ingestion/`, `services/transcription/`, `services/ocr/`, `services/enrichment/`, `services/profiling/`, `services/indexer/`
- In each service: `main.py`, `requirements.txt`, `tests/`, `tests/fixtures/`

**Acceptance criteria**
- All service directories exist and are importable Python modules.
- Placeholder worker entrypoints run without syntax errors.

---

### T-002: Add Python tooling and formatting standards
**Goal:** Standardize formatting, linting, and typing.

**Deliverables**
- Root configs for `black` (line length 100), `ruff`, and pytest
- Optional pre-commit config for local workflow

**Acceptance criteria**
- `black --check .` and `ruff check .` pass on scaffold.

---

### T-003: Build shared `common` package
**Goal:** Avoid duplicated queue/storage/logging code.

**Deliverables**
- `common/models.py` (Pydantic envelope + shared schemas)
- `common/queue.py` (Celery app + publish helper)
- `common/storage.py` (MinIO utility wrapper)
- `common/logging.py` (JSON logger setup)
- `common/errors.py` (retry/DLQ helpers)

**Acceptance criteria**
- Every service imports from `common` instead of reimplementing clients.
- Shared envelope includes `event_id`, `event_type`, `timestamp`, `source_id`.

---

## Epic 1 — Infrastructure & Local Runtime

### T-010: Create `docker-compose.yml` for local stack
**Goal:** Run full pipeline locally.

**Deliverables**
- Services: Redis, MinIO, Elasticsearch, Kibana
- Worker containers for all six services
- Shared network + environment wiring

**Acceptance criteria**
- `docker compose up` starts core infra successfully.
- All workers can resolve Redis/MinIO/Elasticsearch hostnames.

---

### T-011: Add MinIO bootstrap script
**Goal:** Ensure required buckets are created automatically.

**Deliverables**
- Script to create buckets: `raw-media`, `processed`, `frames`

**Acceptance criteria**
- Script is idempotent (safe to run multiple times).

---

### T-012: Environment variable management
**Goal:** Provide consistent local configuration.

**Deliverables**
- `.env.example` containing required variables per service
- Service-specific env validation on startup

**Acceptance criteria**
- Missing required env vars fail fast with readable errors.

---

## Epic 2 — Contracts & Data Validation

### T-020: Implement Pydantic models from `docs/DATA_MODELS.md`
**Goal:** Enforce contract-first processing.

**Deliverables**
- Models for `MediaObject`, `TranscriptSegment`, `OcrFrame`, `ContentDocument`, `EnrichedDocument`
- Event envelope model

**Acceptance criteria**
- Serialization/deserialization tests pass for fixtures.
- Invalid payloads raise validation errors.

---

### T-021: Add schema fixture test suite
**Goal:** Prevent model drift between services.

**Deliverables**
- JSON fixtures for each model
- Contract tests for all fixtures in CI

**Acceptance criteria**
- Fixtures validate in all services that consume them.

---

## Epic 3 — Ingestion Service (Phase 1: YouTube)

### T-030: Build ingestion worker skeleton
**Goal:** Consume ingestion jobs and publish ingested events.

**Deliverables**
- Celery worker consuming `ingestion.jobs`
- `BaseIngestionAdapter` interface
- Queue publisher to `media.ingested`

**Acceptance criteria**
- Receives job event and emits valid `media.ingested` envelope.

---

### T-031: Implement YouTube adapter (`yt-dlp`)
**Goal:** Fetch media metadata and audio/video files.

**Deliverables**
- Metadata extraction and media download logic
- Mapping to `MediaObject`
- MinIO writes for original files + metadata JSON

**Acceptance criteria**
- Given a valid YouTube URL, writes expected files to MinIO and emits `MediaObject`.

---

### T-032: Ingestion error handling + DLQ
**Goal:** Make ingestion resilient.

**Deliverables**
- Retry policy for transient network/storage failures
- Dead-letter publish for unsupported/unavailable sources

**Acceptance criteria**
- Retries capped; failures arrive in dead-letter queue with reason and traceback metadata.

---

## Epic 4 — Transcription Service

### T-040: Implement transcription worker pipeline
**Goal:** Produce transcript segments from ingested audio/video.

**Deliverables**
- Fetch media from MinIO
- Audio normalization (`ffmpeg`, 16kHz mono)
- WhisperX transcription
- Save `transcript.json` to `processed/{media_id}`

**Acceptance criteria**
- Valid `TranscriptSegment[]` produced and stored.

---

### T-041: Add speaker diarization integration
**Goal:** Assign speaker IDs to transcript segments.

**Deliverables**
- pyannote diarization pipeline
- Segment-speaker overlap assignment

**Acceptance criteria**
- Segments include `speaker_id` and satisfy schema.

---

### T-042: Publish content extraction event
**Goal:** Trigger downstream enrichment.

**Deliverables**
- Publish `content.extracted` envelope after transcript write

**Acceptance criteria**
- Event references transcript payload location.

---

## Epic 5 — OCR Service

### T-050: Implement frame extraction strategy
**Goal:** Extract meaningful frames efficiently.

**Deliverables**
- Scene-detection extraction + supplemental interval extraction
- Frame deduplication

**Acceptance criteria**
- Number of processed frames is reduced vs naive extraction.

---

### T-051: Add OCR pipeline with pre-filtering
**Goal:** Extract text from candidate frames/pages.

**Deliverables**
- OpenCV text-density pre-filter
- PaddleOCR run and post-processing
- Store frame thumbnails in `frames` bucket

**Acceptance criteria**
- Outputs valid `OcrFrame[]` including `full_text` and `text_blocks`.

---

### T-052: PDF page OCR support
**Goal:** Process page images generated during ingestion.

**Deliverables**
- Read page images from MinIO
- OCR pages into `OcrFrame` with page-index mapping

**Acceptance criteria**
- PDF source generates OCR output with expected indexing.

---

## Epic 6 — Enrichment Service

### T-060: Build content assembler
**Goal:** Build `ContentDocument` from transcript/OCR/body.

**Deliverables**
- Merge and deduplicate text using section markers
- Save `content.json`

**Acceptance criteria**
- `full_text` assembled in required order and validates.

---

### T-061: Add extraction coordination logic
**Goal:** Run enrichment only when transcript + OCR artifacts are available.

**Deliverables**
- Peer-output existence check in MinIO
- delayed re-queue with capped retries

**Acceptance criteria**
- No enrichment starts with missing required inputs.

---

### T-062: Integrate NLP components
**Goal:** Produce complete `EnrichedDocument`.

**Deliverables**
- Language detection
- spaCy NER
- KeyBERT keywords
- Sentiment (document + per-segment)
- Topic assignment from taxonomy YAML
- Embedding generation

**Acceptance criteria**
- `enriched.json` includes all required fields and passes schema validation.

---

### T-063: Publish `content.enriched`
**Goal:** Fan out to indexer and profiling.

**Deliverables**
- Event publishing after enriched payload write

**Acceptance criteria**
- Both terminal consumers can process emitted events independently.

---

## Epic 7 — Indexer Service

### T-070: Add Elasticsearch index bootstrap
**Goal:** Ensure required index mappings exist.

**Deliverables**
- Mapping JSON files in `services/indexer/mappings/`
- Startup `ensure_indices` logic

**Acceptance criteria**
- Missing indices are created with expected mappings.

---

### T-071: Implement document-level indexing
**Goal:** Upsert one `media_documents` record per media item.

**Deliverables**
- Transformation from `EnrichedDocument` to document record
- Deterministic ID = `media_id`

**Acceptance criteria**
- Re-index is idempotent and overwrites same record safely.

---

### T-072: Implement segment bulk indexing
**Goal:** Index transcript and OCR units for granular retrieval.

**Deliverables**
- Build segment records from transcript + OCR
- Bulk indexing via Elasticsearch helpers

**Acceptance criteria**
- Deterministic segment IDs avoid duplicates on retries.

---

## Epic 8 — Profiling Service

### T-080: Implement entity profile upsert flow
**Goal:** Maintain aggregate profiles per entity.

**Deliverables**
- Entity extraction + normalization
- Profile create/update with `retry_on_conflict`

**Acceptance criteria**
- `mention_count`, `media_ids`, `first_seen`, `last_seen` update correctly.

---

### T-081: Rule-based alias resolution
**Goal:** Resolve obvious alias variants in Phase 1.

**Deliverables**
- `aliases.yaml` support
- Canonicalization + alias lookup utility

**Acceptance criteria**
- Known aliases map to canonical entity IDs deterministically.

---

### T-082: Co-occurrence graph updates
**Goal:** Track entity relationships from shared appearances.

**Deliverables**
- Pairwise co-occurrence updates per document

**Acceptance criteria**
- Frequent co-occurrences accumulate consistently across runs.

---

## Epic 9 — Testing & Quality Gates

### T-090: Unit tests for each service component
**Goal:** Validate adapters/processors in isolation.

**Deliverables**
- Unit tests for adapters, transformers, and validators

**Acceptance criteria**
- Unit tests pass for all implemented modules.

---

### T-091: Service integration tests with fixtures
**Goal:** Validate each worker with representative payloads.

**Deliverables**
- Integration tests in `services/{name}/tests/`
- Fixture media/payloads under `tests/fixtures/`

**Acceptance criteria**
- Each service has at least one end-to-end worker test.

---

### T-092: Pipeline smoke test (ingestion → enrichment → index)
**Goal:** Validate cross-service interoperability.

**Deliverables**
- Script/test that runs one sample media through full pipeline

**Acceptance criteria**
- Final docs exist in Elasticsearch and profile index.

---

## Epic 10 — Observability, Reliability, Security

### T-100: Structured logging standardization
**Goal:** Improve debugging across async stages.

**Deliverables**
- JSON logs with event/service/media IDs

**Acceptance criteria**
- Logs correlate across services using shared identifiers.

---

### T-101: Metrics and health checks
**Goal:** Instrument pipeline performance and failure rates.

**Deliverables**
- Per-service metrics endpoints
- Processing latency/error counters

**Acceptance criteria**
- Metrics visible and scrapeable in local environment.

---

### T-102: DLQ monitoring and replay helper
**Goal:** Make failures operationally manageable.

**Deliverables**
- Queue depth alerting strategy
- replay script/tool for dead-letter events

**Acceptance criteria**
- Failed events can be replayed after remediation.

---

## Epic 11 — Expansion Tasks (Roadmap Alignment)

### T-110: Add Twitter/X adapter
### T-111: Add Substack adapter
### T-112: Add RSS + podcast feed adapter
### T-113: Add PDF and plain text ingestion adapters

**Acceptance criteria (all):**
- Adapter-specific unit tests + one integration test each.

---

## Suggested execution order (critical path)

1. T-001 → T-003 (scaffold + shared package)
2. T-010 → T-012 (local infra)
3. T-020 → T-021 (contracts)
4. T-030 → T-032 (ingestion YouTube)
5. T-040 → T-042 (transcription)
6. T-050 → T-052 (OCR)
7. T-060 → T-063 (enrichment)
8. T-070 → T-072 (indexer)
9. T-080 → T-082 (profiling)
10. T-090 → T-102 (quality + ops)
11. T-110+ (source expansion)

---

## Definition of Done (global)

A task is Done only when:
- Code merged with tests.
- Contract validation passes for input/output payloads.
- Idempotency is verified for retries/reprocessing.
- Logging includes enough metadata for traceability.
- Relevant docs updated.
