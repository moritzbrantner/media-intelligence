# Service Spec: Source Watcher

## Responsibility

Monitor subscribed sources for new content and emit notifications that kick off downstream ingestion or direct processing.

This service is designed for polling-based sources like:

- Websites (ETag / Last-Modified)
- Blogs and RSS/Atom feeds
- YouTube channels (via channel RSS)

---

## Input: Source Subscription

```json
{
  "source_id": "my-tech-blog",
  "source_type": "blog",
  "source_url": "https://example.com/feed.xml",
  "target_services": [
    "http://ingestion:8080/v1/jobs",
    "http://archiver:8080/v1/archive"
  ],
  "metadata": {
    "channel_id": "UC_x5XG1OV2P6uZZ5FSM9Ttw"
  }
}
```

---

## Output: `source.updated`

```json
{
  "event_type": "source.updated",
  "source_id": "my-tech-blog",
  "source_type": "blog",
  "source_url": "https://example.com/feed.xml",
  "update_id": "entry-123",
  "title": "New article title",
  "published_at": "2024-02-20T10:00:00+00:00",
  "payload": {
    "link": "https://example.com/posts/new-article"
  }
}
```

---

## Detection Strategy

### Website
- Perform `HEAD` requests with `If-None-Match` and `If-Modified-Since` when state exists.
- Treat changes in ETag or Last-Modified as updates.

### RSS/Blog
- Fetch feed XML and read latest `<item>` / `<entry>`.
- Track the latest `guid`/`id` to detect new content.

### YouTube Channel
- Resolve channel RSS feed from `channel_id` if provided.
- Detect newest upload using the same RSS entry strategy.

---

## Notification Strategy

- On update, send a JSON webhook to each `target_services` endpoint.
- Each notification includes source metadata and the discovered content payload.
- The consumer may either:
  1) trigger ingestion by URL, or
  2) process source material directly.

---

## Local Mock/HTTP API

The service entrypoint (`python -m services.source_watcher.main`) exposes:

- `GET /health` — liveness check
- `GET /v1/state` — in-memory check state by `source_id`
- `POST /v1/check` — submit one subscription and run a single check

`POST /v1/check` returns:
- `{"status": "no_update"}` when no new item is found
- `{"status": "update_detected", "update": ...}` when an update is discovered and dispatched
