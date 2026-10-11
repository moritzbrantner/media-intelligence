# Service Spec: Source Watcher

## Responsibility

Monitor subscribed sources for new content and emit notifications that kick off downstream ingestion or direct processing.

This service is designed for polling-based sources like:

- Websites (ETag / Last-Modified)
- Blogs and RSS/Atom feeds
- YouTube channels (RSS or `yt-dlp` channel extraction)

X/Twitter accounts and Facebook pages are not supported yet: `yt-dlp` has no
timeline extractor for them, so they need a dedicated timeline-capable provider.

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
- `youtube_channel` uses channel RSS from `channel_id` when provided.
- `youtube` invokes `yt-dlp --dump-single-json --flat-playlist --playlist-end 1 --extractor-args youtubetab:approximate_date` against the channel upload tab of `source_url` (`/videos` or `/shorts`), which lists the newest upload first. `/streams` is rejected because a scheduled future broadcast can stay first there. A root channel URL such as `https://www.youtube.com/@name` is normalized to its `/videos` tab; other tabs and non-channel URLs are rejected with HTTP 400.
- The newest entry id is the `update_id`; the payload carries the item's `webpage_url` (flat entries are not fully extracted, so ingestion resolves the media from that page) plus extractor metadata.
- Playlist URLs (`/playlist` or a `list=` parameter) are rejected with HTTP 400 because their order does not reflect upload date; so is a `source_url` that resolves to a single item.
- If yt-dlp is missing, exits non-zero, times out or prints invalid JSON, `/v1/check` answers HTTP 502 with `{"error": "source_check_failed", "detail": ...}`: a transient check failure, distinct from a rejected subscription (400).

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


### Profile-building metadata

For `yt-dlp`-backed channels, the update payload includes normalized channel fields (`channel_name`, `channel_id`, `uploader_id`) so Profiling can build stable channel profiles across platforms, alongside `platform` and `raw_entry`.
