# Archiving Service

## Purpose
Run a dedicated, self-hosted archive instance to preserve web sources processed by the platform.

Recommended implementation: **ArchiveBox** as a standalone service integrated via API/CLI jobs.

---

## Why include archiving
- Reproducibility: preserve a stable snapshot used for extraction
- Compliance/audit: retain evidence for source-at-time-of-processing
- Resilience: source content remains available after edits/deletions

---

## Integration Model
1. Ingestion or Web Parsing emits URL + metadata
2. Archiving Service submits URL to ArchiveBox
3. ArchiveBox stores snapshot assets (HTML/media/WARC if enabled)
4. Service writes archive identifier and snapshot paths back to metadata
5. Optional event: `content.archived`

---

## Minimal Metadata Fields
```json
{
  "source_url": "https://example.com/post",
  "archive_id": "archivebox-uuid",
  "archived_at": "2026-01-01T00:00:00Z",
  "snapshot_paths": ["/data/archive/..."],
  "warc_path": "/data/archive/warc/..."
}
```

---

## Operational Notes
- Run ArchiveBox with persistent volume storage
- Schedule periodic re-archives for high-value domains
- Keep archiving asynchronous to avoid blocking extraction throughput
- Add retention and access-control policies appropriate for your legal requirements
