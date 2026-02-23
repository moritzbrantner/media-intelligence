"""Service orchestration for source update monitoring."""

from __future__ import annotations

from dataclasses import asdict
from datetime import datetime
from typing import Any

from .models import CheckState, SourceSubscription, SourceUpdate
from .notifier import Notifier
from .providers import SourceProvider


class SourceWatcherService:
    """Checks source subscriptions and dispatches updates downstream."""

    def __init__(self, providers: list[SourceProvider], notifier: Notifier) -> None:
        self.providers = providers
        self.notifier = notifier
        self._state: dict[str, CheckState] = {}

    def check_subscription(self, subscription: SourceSubscription) -> SourceUpdate | None:
        provider = self._resolve_provider(subscription.source_type)
        state = self._state.get(subscription.source_id, CheckState())
        update_payload, next_state = provider.check_for_update(subscription, state)
        self._state[subscription.source_id] = next_state

        if update_payload is None:
            return None

        update = SourceUpdate(
            source_id=subscription.source_id,
            source_type=subscription.source_type,
            source_url=subscription.source_url,
            update_id=update_payload["update_id"],
            title=update_payload.get("title"),
            published_at=update_payload.get("published_at"),
            payload=update_payload.get("payload", {}),
        )
        self.notifier.notify(subscription, update)
        return update

    def list_states(self) -> dict[str, dict[str, Any]]:
        """Return current watcher state for observability/debugging."""
        return {source_id: asdict(state) for source_id, state in self._state.items()}

    def _resolve_provider(self, source_type: str) -> SourceProvider:
        for provider in self.providers:
            if provider.supports(source_type):
                return provider
        raise ValueError(f"Unsupported source_type '{source_type}'")


def parse_subscription(payload: dict[str, Any]) -> SourceSubscription:
    """Convert JSON payload into a typed subscription object."""
    target_services = payload.get("target_services")
    if not isinstance(target_services, list) or not all(isinstance(s, str) for s in target_services):
        raise ValueError("target_services must be a list of URLs")

    return SourceSubscription(
        source_id=str(payload["source_id"]),
        source_type=str(payload["source_type"]),
        source_url=str(payload["source_url"]),
        target_services=target_services,
        metadata=payload.get("metadata", {}),
    )


def serialize_update(update: SourceUpdate) -> dict[str, Any]:
    """Serialize update model for JSON responses."""
    return {
        "source_id": update.source_id,
        "source_type": update.source_type,
        "source_url": update.source_url,
        "update_id": update.update_id,
        "title": update.title,
        "published_at": _iso_or_none(update.published_at),
        "detected_at": update.detected_at.isoformat(),
        "payload": update.payload,
    }


def _iso_or_none(value: datetime | None) -> str | None:
    return value.isoformat() if value else None
