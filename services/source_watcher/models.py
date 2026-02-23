"""Data models for source watch subscriptions and detected updates."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Any


@dataclass(slots=True)
class SourceSubscription:
    """A single source endpoint that should be monitored for new content."""

    source_id: str
    source_type: str
    source_url: str
    target_services: list[str]
    metadata: dict[str, Any] = field(default_factory=dict)


@dataclass(slots=True)
class SourceUpdate:
    """An update discovered for a monitored source."""

    source_id: str
    source_type: str
    source_url: str
    update_id: str
    published_at: datetime | None
    title: str | None
    payload: dict[str, Any]
    detected_at: datetime = field(default_factory=lambda: datetime.now(timezone.utc))


@dataclass(slots=True)
class CheckState:
    """Persisted state that allows update detection to be idempotent."""

    last_update_id: str | None = None
    etag: str | None = None
    last_modified: str | None = None
