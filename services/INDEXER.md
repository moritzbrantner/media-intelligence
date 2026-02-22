# Service Spec: Indexer

## Responsibility

Write enriched documents to Elasticsearch, ensuring correct index mappings, handling updates to existing documents idempotently, and maintaining index health.

---

## Interface

### Input

Consumes from `queue: content.enriched`.

Reads the `EnrichedDocument` from MinIO: `minio://processed/{media_id}/enriched.json`.

### Output

Documents written to Elasticsearch indices:
- `media_documents` — one document per media object
- `media_segments` — one document per transcript segment or OCR frame

### Publishes To

Nothing. Terminal stage (runs in parallel with Profiling).

---

## Index Setup

On service startup, the Indexer must check that all required indices exist with correct mappings. If they do not, create them.

Index mappings are defined in `docs/DATA_MODELS.md`. Store them as JSON files in `services/indexer/mappings/`:

```
services/indexer/mappings/
├── media_documents.json
├── media_segments.json
└── entity_profiles.json
```

```python
def ensure_indices(es_client):
    for index_name in ["media_documents", "media_segments", "entity_profiles"]:
        if not es_client.indices.exists(index=index_name):
            with open(f"mappings/{index_name}.json") as f:
                mapping = json.load(f)
            es_client.indices.create(index=index_name, body=mapping)
```

---

## Processing Steps

1. **Load** `EnrichedDocument` from MinIO
2. **Build** `media_documents` record (document-level fields)
3. **Upsert** to `media_documents` using `media_id` as the document ID
4. **Build** segment records from `transcript_segments` and `ocr_frames`
5. **Bulk upsert** all segments to `media_segments`
6. **Log** success with document counts

---

## Document Transformation

### media_documents record

```python
def build_document_record(enriched: dict) -> dict:
    return {
        "media_id": enriched["media_id"],
        "source_type": enriched["source_type"],
        "source_url": enriched.get("source_url"),
        "title": enriched.get("title"),
        "description": enriched.get("description"),
        "body_text": enriched.get("body_text"),
        "author_name": enriched.get("author", {}).get("name"),
        "author_handle": enriched.get("author", {}).get("handle"),
        "channel_name": enriched.get("channel", {}).get("name"),
        "published_at": enriched.get("published_at"),
        "language": enriched.get("language"),
        "topics": [t["label"] for t in enriched.get("topics", [])],
        "keywords": [k["phrase"] for k in enriched.get("keywords", [])],
        "entities": [
            {"text": e["text"], "label": e["label"]}
            for e in enriched.get("entities", [])
        ],
        "sentiment_label": enriched.get("sentiment", {}).get("document_level", {}).get("label"),
        "sentiment_score": enriched.get("sentiment", {}).get("document_level", {}).get("score"),
        "embedding": enriched.get("embedding"),
        "indexed_at": datetime.utcnow().isoformat()
    }
```

### media_segments records

```python
def build_segment_records(enriched: dict) -> list[dict]:
    records = []

    # Transcript segments
    for seg in enriched.get("transcript_segments", []):
        seg_sentiment = next(
            (s for s in enriched.get("sentiment", {}).get("per_segment", [])
             if s["segment_index"] == seg["segment_index"]),
            None
        )
        records.append({
            "_id": f"{enriched['media_id']}_seg_{seg['segment_index']}",
            "media_id": enriched["media_id"],
            "segment_type": "transcript",
            "speaker_id": seg.get("speaker_id"),
            "speaker_name": seg.get("speaker_name"),
            "start_seconds": seg["start_seconds"],
            "end_seconds": seg["end_seconds"],
            "text": seg["text"],
            "sentiment_label": seg_sentiment["label"] if seg_sentiment else None,
            "sentiment_score": seg_sentiment["score"] if seg_sentiment else None,
            "published_at": enriched.get("published_at"),
            "source_type": enriched["source_type"]
        })

    # OCR frames
    for frame in enriched.get("ocr_frames", []):
        if frame.get("full_text", "").strip():
            records.append({
                "_id": f"{enriched['media_id']}_ocr_{frame['frame_index']}",
                "media_id": enriched["media_id"],
                "segment_type": "ocr",
                "timestamp_seconds": frame["timestamp_seconds"],
                "text": frame["full_text"],
                "published_at": enriched.get("published_at"),
                "source_type": enriched["source_type"]
            })

    return records
```

---

## Bulk Indexing

Use Elasticsearch's `helpers.bulk` for segment indexing:

```python
from elasticsearch import helpers

def index_segments(es_client, records: list[dict], index: str):
    actions = [
        {
            "_op_type": "index",
            "_index": index,
            "_id": record.pop("_id"),
            "_source": record
        }
        for record in records
    ]
    success, errors = helpers.bulk(es_client, actions, raise_on_error=False)
    if errors:
        logger.error("Bulk index errors: %s", errors)
    return success, errors
```

---

## Idempotency

All writes use `index` (not `create`) operations with a deterministic `_id`. Re-indexing the same `media_id` safely overwrites previous data.

For the `media_documents` index, use:
```python
es_client.index(
    index="media_documents",
    id=enriched["media_id"],
    document=document_record
)
```

---

## Index Lifecycle Management

For large deployments, consider time-based indices with an alias:

```
media_segments_2024_q1 ─┐
media_segments_2024_q2 ─┤── alias: media_segments (write to latest)
media_segments_2024_q3 ─┘
```

This enables archiving old data and rolling index management. Implement in Phase 7.

---

## Environment Variables

| Variable | Description |
|---|---|
| `REDIS_URL` | Queue connection |
| `MINIO_ENDPOINT` | MinIO host |
| `MINIO_ACCESS_KEY` | MinIO credentials |
| `MINIO_SECRET_KEY` | MinIO credentials |
| `ELASTICSEARCH_URL` | Elasticsearch URL |
| `ELASTICSEARCH_API_KEY` | API key for auth (if enabled) |
| `BULK_CHUNK_SIZE` | Documents per bulk request (default: `500`) |
| `LOG_LEVEL` | Logging level |
