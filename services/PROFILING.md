# Service Spec: Profiling

## Responsibility

Build and continuously update unified profiles for people, organizations, and channels by aggregating information across all processed media. A profile answers: "Who is this entity, where do they appear, what do they talk about, and who do they associate with?"

---

## Interface

### Input

Consumes from `queue: content.enriched`.

Reads the `EnrichedDocument` from MinIO: `minio://processed/{media_id}/enriched.json`.

### Output

Upserted `EntityProfile` documents in Elasticsearch index `entity_profiles`.

### Publishes To

Nothing. This is a terminal stage.

---

## What a Profile Contains

See `docs/DATA_MODELS.md` for the full `EntityProfile` schema. At a high level:

- **Identity**: canonical name, aliases, entity type
- **Sources**: which platforms/channels they appear on (with handles, URLs)
- **Activity**: list of media IDs they appear in, first/last seen dates, mention count
- **Thematic fingerprint**: frequent topics, frequent keywords
- **Social graph**: frequent co-occurrences with other entities
- **Sentiment profile**: how content featuring them tends to be framed

---

## Processing Steps

For each incoming `EnrichedDocument`:

1. **Extract entity mentions** from `entities` array
2. **For each entity**:
   a. Compute `entity_id` = `{entity_type}_{slugify(canonical_name)}`
   b. Fetch existing profile from Elasticsearch (may not exist yet)
   c. Merge new data into the profile
   d. Upsert profile back to Elasticsearch with `retry_on_conflict: 3`
3. **Update co-occurrence counts** between entities in the same document
4. **Check if the document author/channel** has a profile — create/update a `CHANNEL` profile

---

## Entity Resolution

Raw NER output is messy. The same person may appear as "Elon Musk", "Elon", "@elonmusk", "Mr. Musk". Resolution strategy:

### Phase 1: Rule-based
- Normalize: strip titles (Mr., Dr., CEO), lowercase, strip punctuation
- Alias dictionary: maintain a YAML file `enrichment/aliases.yaml`:
  ```yaml
  aliases:
    "elon musk":
      canonical: "Elon Musk"
      entity_type: PERSON
      known_aliases: ["elon", "@elonmusk", "musk"]
    "anthropic":
      canonical: "Anthropic"
      entity_type: ORG
      known_aliases: ["anthropic ai", "anthropic pbc"]
  ```
- Before creating a new profile, check if the normalized name matches any known alias

### Phase 2: Embedding-based similarity (future)
- If no alias match found, embed the entity name and compare cosine similarity against existing profile canonical names
- If similarity > 0.92, treat as same entity

---

## Channel Profiling

Every ingested media object has an author and/or channel. The Profiling service should maintain channel profiles that aggregate across all content from that source.

A channel profile tracks:
- All media IDs from this channel
- Topics the channel covers most frequently
- Entities the channel mentions most frequently
- Sentiment trend over time
- Known guests/collaborators (co-occurring PERSON entities)

**Channel identity**:
- YouTube: use `channel_id` from metadata as the stable ID
- Twitter: use `author_id`
- Substack: use the newsletter subdomain or custom domain

---

## Co-occurrence Graph

When two entities appear in the same document, increment their co-occurrence count:

```python
from itertools import combinations

entity_names = [e["text"] for e in enriched_doc["entities"]]
for a, b in combinations(set(entity_names), 2):
    update_cooccurrence(entity_id_a=slugify(a), entity_id_b=slugify(b), count_increment=1)
```

Store co-occurrences in the `entity_profiles` index under `frequent_cooccurrences`. This data can later power a graph visualization (e.g. Neo4j or Kibana graph).

---

## Elasticsearch Upsert Pattern

Use Elasticsearch's `update` API with a painless script for atomic increments:

```python
es.update(
    index="entity_profiles",
    id=entity_id,
    body={
        "script": {
            "source": """
                ctx._source.mention_count += 1;
                ctx._source.last_seen = params.last_seen;
                if (!ctx._source.media_ids.contains(params.media_id)) {
                    ctx._source.media_ids.add(params.media_id);
                }
                for (kw in params.keywords) {
                    // merge keywords with frequency count
                }
            """,
            "params": {
                "last_seen": enriched_doc["enriched_at"],
                "media_id": enriched_doc["media_id"],
                "keywords": enriched_doc["keywords"]
            }
        },
        "upsert": {
            "entity_id": entity_id,
            "entity_type": entity_type,
            "canonical_name": canonical_name,
            "mention_count": 1,
            "first_seen": enriched_doc["enriched_at"],
            "last_seen": enriched_doc["enriched_at"],
            "media_ids": [enriched_doc["media_id"]],
            "sources": [],
            "frequent_topics": [],
            "frequent_keywords": [],
            "frequent_cooccurrences": [],
            "updated_at": enriched_doc["enriched_at"]
        }
    },
    retry_on_conflict=3
)
```

---

## Speaker Name Resolution

The Transcription service assigns opaque IDs (`SPEAKER_00`). Profiling can resolve these using:

1. **Name mentions near the start**: If a transcript segment early in the content says "Welcome, John Smith" followed by `SPEAKER_01` speaking, tag that speaker as John Smith
2. **Host introduction pattern**: "My guest today is {NAME}" → next speaker is {NAME}
3. **Manual override API**: Allow users to correct speaker assignments via the API

When resolved, update the `TranscriptSegment` records in `media_segments` index.

---

## Environment Variables

| Variable | Description |
|---|---|
| `REDIS_URL` | Queue connection |
| `MINIO_ENDPOINT` | MinIO host |
| `MINIO_ACCESS_KEY` | MinIO credentials |
| `MINIO_SECRET_KEY` | MinIO credentials |
| `ELASTICSEARCH_URL` | Elasticsearch URL |
| `ALIASES_CONFIG` | Path to `aliases.yaml` |
| `SIMILARITY_THRESHOLD` | Entity resolution similarity cutoff (default: `0.92`) |
