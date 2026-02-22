# Data Models

This document is the **source of truth** for all data structures exchanged between services and stored in Elasticsearch. All services must conform to these schemas.

---

## Core Pipeline Objects

### MediaObject
Created by the Ingestion Service. Describes a raw piece of media fetched from a source.

```json
{
  "id": "string (uuid-v4)",
  "source_type": "youtube | twitter | substack | pdf | text | rss",
  "source_url": "string",
  "source_id": "string (platform-native ID, e.g. YouTube video ID)",
  "title": "string | null",
  "description": "string | null",
  "author": {
    "id": "string | null",
    "name": "string | null",
    "handle": "string | null",
    "profile_url": "string | null"
  },
  "channel": {
    "id": "string | null",
    "name": "string | null",
    "url": "string | null"
  },
  "published_at": "ISO-8601 | null",
  "fetched_at": "ISO-8601",
  "duration_seconds": "number | null",
  "language": "string | null (ISO 639-1)",
  "tags": ["string"],
  "media_files": [
    {
      "type": "video | audio | image | document | text",
      "storage_path": "minio://bucket/path/to/file",
      "mime_type": "string",
      "size_bytes": "number"
    }
  ],
  "raw_metadata": {}
}
```

---

### TranscriptSegment
Produced by the Transcription Service. Represents one speaker turn in a transcript.

```json
{
  "media_id": "string (ref to MediaObject.id)",
  "segment_index": "number",
  "speaker_id": "string (e.g. SPEAKER_00, SPEAKER_01)",
  "speaker_name": "string | null (resolved later by profiling)",
  "start_seconds": "number",
  "end_seconds": "number",
  "text": "string",
  "language": "string (ISO 639-1)",
  "confidence": "number (0.0–1.0)"
}
```

---

### OcrFrame
Produced by the OCR Service. Represents a video frame where text was detected.

```json
{
  "media_id": "string",
  "frame_index": "number",
  "timestamp_seconds": "number",
  "scene_change": "boolean",
  "text_blocks": [
    {
      "text": "string",
      "confidence": "number",
      "bounding_box": { "x": 0, "y": 0, "width": 0, "height": 0 }
    }
  ],
  "full_text": "string (concatenation of all text_blocks)",
  "storage_path": "minio://bucket/path/to/frame.jpg | null"
}
```

---

### ContentDocument
The unified representation passed to the Enrichment Service. Merges all extracted text from a single media object.

```json
{
  "media_id": "string",
  "source_type": "string",
  "title": "string | null",
  "description": "string | null",
  "author": { "...": "see MediaObject.author" },
  "channel": { "...": "see MediaObject.channel" },
  "published_at": "ISO-8601 | null",
  "language": "string | null",
  "transcript_segments": ["TranscriptSegment"],
  "ocr_frames": ["OcrFrame"],
  "body_text": "string | null (for articles, PDFs, tweets, etc.)",
  "full_text": "string (merged text from all sources — used for NLP)"
}
```

---

### EnrichedDocument
Output of the Enrichment Service. Ready for indexing and profiling.

```json
{
  "media_id": "string",
  "source_type": "string",
  "title": "string | null",
  "author": { "...": "see MediaObject.author" },
  "channel": { "...": "see MediaObject.channel" },
  "published_at": "ISO-8601 | null",
  "language": "string",

  "transcript_segments": ["TranscriptSegment"],
  "ocr_frames": ["OcrFrame"],
  "body_text": "string | null",

  "entities": [
    {
      "text": "string",
      "label": "PERSON | ORG | GPE | PRODUCT | EVENT | NORP | WORK_OF_ART | LAW | ...",
      "start_char": "number",
      "end_char": "number",
      "source": "transcript | ocr | body | title"
    }
  ],

  "keywords": [
    {
      "phrase": "string",
      "score": "number",
      "source": "transcript | ocr | body"
    }
  ],

  "topics": [
    {
      "label": "string",
      "score": "number"
    }
  ],

  "sentiment": {
    "document_level": {
      "label": "positive | negative | neutral",
      "score": "number"
    },
    "per_segment": [
      {
        "segment_index": "number",
        "label": "positive | negative | neutral",
        "score": "number"
      }
    ]
  },

  "embedding": ["number (768 or 1536 dimensional vector)"],

  "enriched_at": "ISO-8601"
}
```

---

## Elasticsearch Index Schemas

### Index: `media_documents`
One document per ingested media object. Used for document-level search.

```json
{
  "mappings": {
    "properties": {
      "media_id":       { "type": "keyword" },
      "source_type":    { "type": "keyword" },
      "source_url":     { "type": "keyword" },
      "title":          { "type": "text", "analyzer": "english" },
      "description":    { "type": "text", "analyzer": "english" },
      "body_text":      { "type": "text", "analyzer": "english" },
      "author_name":    { "type": "keyword" },
      "author_handle":  { "type": "keyword" },
      "channel_name":   { "type": "keyword" },
      "published_at":   { "type": "date" },
      "language":       { "type": "keyword" },
      "topics":         { "type": "keyword" },
      "keywords":       { "type": "keyword" },
      "entities": {
        "type": "nested",
        "properties": {
          "text":  { "type": "keyword" },
          "label": { "type": "keyword" }
        }
      },
      "sentiment_label": { "type": "keyword" },
      "sentiment_score": { "type": "float" },
      "embedding": {
        "type": "dense_vector",
        "dims": 768,
        "index": true,
        "similarity": "cosine"
      }
    }
  }
}
```

---

### Index: `media_segments`
One document per transcript segment or OCR frame. Enables fine-grained retrieval ("find me the exact moment someone mentions Bitcoin").

```json
{
  "mappings": {
    "properties": {
      "media_id":          { "type": "keyword" },
      "segment_type":      { "type": "keyword" },
      "speaker_id":        { "type": "keyword" },
      "speaker_name":      { "type": "keyword" },
      "start_seconds":     { "type": "float" },
      "end_seconds":       { "type": "float" },
      "timestamp_seconds": { "type": "float" },
      "text":              { "type": "text", "analyzer": "english" },
      "sentiment_label":   { "type": "keyword" },
      "sentiment_score":   { "type": "float" },
      "published_at":      { "type": "date" },
      "source_type":       { "type": "keyword" },
      "embedding": {
        "type": "dense_vector",
        "dims": 768,
        "index": true,
        "similarity": "cosine"
      }
    }
  }
}
```

---

### Index: `entity_profiles`
One document per unique entity (person, organization, channel). Continuously updated by the Profiling Service.

```json
{
  "mappings": {
    "properties": {
      "entity_id":       { "type": "keyword" },
      "entity_type":     { "type": "keyword" },
      "canonical_name":  { "type": "text", "fields": { "keyword": { "type": "keyword" } } },
      "aliases":         { "type": "keyword" },
      "description":     { "type": "text" },
      "sources": {
        "type": "nested",
        "properties": {
          "source_type":  { "type": "keyword" },
          "handle":       { "type": "keyword" },
          "url":          { "type": "keyword" },
          "platform_id":  { "type": "keyword" }
        }
      },
      "media_ids":        { "type": "keyword" },
      "mention_count":    { "type": "integer" },
      "first_seen":       { "type": "date" },
      "last_seen":        { "type": "date" },
      "frequent_topics":  { "type": "keyword" },
      "frequent_keywords": { "type": "keyword" },
      "frequent_cooccurrences": {
        "type": "nested",
        "properties": {
          "entity_id":   { "type": "keyword" },
          "entity_name": { "type": "keyword" },
          "count":       { "type": "integer" }
        }
      },
      "sentiment_summary": {
        "properties": {
          "average_score": { "type": "float" },
          "positive_count": { "type": "integer" },
          "negative_count": { "type": "integer" },
          "neutral_count":  { "type": "integer" }
        }
      },
      "updated_at": { "type": "date" }
    }
  }
}
```

---

## Entity Types

The platform tracks these entity types across all sources:

| Type | Description | Example |
|---|---|---|
| `PERSON` | Named individual | Elon Musk, Satoshi Nakamoto |
| `ORG` | Organization or company | Anthropic, BlackRock |
| `CHANNEL` | Media channel or account | Lex Fridman Podcast, @pmarca |
| `GPE` | Geopolitical entity | Germany, New York |
| `PRODUCT` | Product or technology | Bitcoin, GPT-4 |
| `EVENT` | Named event | ETH Denver, Davos 2024 |
| `NORP` | Nationality, religion, political group | Libertarian, Austrian School |
| `WORK_OF_ART` | Books, papers, films | The Bitcoin Standard |

---

## ID Strategy

- `media_id`: `{source_type}_{sha256(source_url)[:12]}`
- `entity_id`: `{entity_type}_{slugify(canonical_name)}`
- Segment IDs: `{media_id}_seg_{segment_index}` / `{media_id}_ocr_{frame_index}`

This ensures IDs are deterministic and re-runs are idempotent.
