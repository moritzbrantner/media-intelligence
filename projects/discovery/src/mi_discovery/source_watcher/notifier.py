"""Notifier implementations for downstream dispatch."""

from __future__ import annotations

from abc import ABC, abstractmethod
from datetime import datetime
from typing import Any

import requests

from .models import SourceSubscription, SourceUpdate


class Notifier(ABC):
    """Dispatch update events to downstream services."""

    @abstractmethod
    def notify(self, subscription: SourceSubscription, update: SourceUpdate) -> None:
        """Send one discovered update."""


class WebhookNotifier(Notifier):
    """Notify target webhooks listed on the subscription."""

    def notify(self, subscription: SourceSubscription, update: SourceUpdate) -> None:
        payload = _serialize_update(update)
        for endpoint in subscription.target_services:
            requests.post(endpoint, json=payload, timeout=15).raise_for_status()


def _serialize_update(update: SourceUpdate) -> dict[str, Any]:
    return {
        "source_id": update.source_id,
        "source_type": update.source_type,
        "source_url": update.source_url,
        "update_id": update.update_id,
        "title": update.title,
        "published_at": _isoformat_or_none(update.published_at),
        "detected_at": update.detected_at.isoformat(),
        "payload": update.payload,
    }


def _isoformat_or_none(value: datetime | None) -> str | None:
    if value is None:
        return None
    return value.isoformat()

