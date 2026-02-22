# Web Parsing Service

## Purpose
Provide a self-hosted, long-term-control parser for websites that extracts:
- Main text
- Outbound and internal links
- Images
- Audio references

This service is designed to fit the existing event-driven pipeline and produce a normalized `WebDocument` payload that downstream Enrichment and Indexer services can consume.

---

## Recommended Stack (Self-Hosted)
- **Fetch/Render**: Playwright (Chromium) with JS rendering fallback
- **Text extraction**: Trafilatura for article/body quality
- **DOM extraction**: BeautifulSoup or lxml for links/media
- **URL normalization**: `urllib.parse` + canonicalization rules

---

## Extraction Contract

```json
{
  "url": "https://example.com/post",
  "canonical_url": "https://example.com/post",
  "title": "Example Post",
  "text": "Main cleaned text...",
  "links": [
    {"url": "https://example.com/about", "anchor": "About", "rel": "nofollow"}
  ],
  "images": [
    {"url": "https://example.com/image.jpg", "alt": "hero image"}
  ],
  "audio": [
    {"url": "https://example.com/audio.mp3", "type": "audio/mpeg", "source": "audio_tag"}
  ],
  "metadata": {
    "lang": "en",
    "published_at": "2026-01-01T00:00:00Z"
  },
  "fetched_at": "2026-01-01T00:00:00Z"
}
```

---

## Audio Discovery Rules
Extract audio from:
1. `<audio src>`
2. `<audio><source src=...>`
3. Direct file links (`.mp3`, `.m4a`, `.wav`, `.ogg`, `.flac`)
4. JSON-LD `AudioObject` entries
5. RSS/Atom enclosure links when present

All discovered URLs should be normalized to absolute URLs and deduplicated.

---

## Queue Integration
- **Consumes**: `queue: media.ingested` and/or source-specific URL discovery queues
- **Publishes**: `queue: content.extracted` with `source_type=website`
- Large payloads are stored in MinIO and referenced via `payload_location`

---

## Reliability Controls
- Idempotency key: canonical URL + content hash
- Retry policy: exponential backoff on network/render failures
- Respect robots.txt and per-domain rate limits
- Store raw HTML snapshot reference for debugging and replay
