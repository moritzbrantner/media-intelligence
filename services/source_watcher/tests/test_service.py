from __future__ import annotations

from datetime import datetime, timezone

from services.source_watcher.models import CheckState, SourceSubscription
from services.source_watcher.service import SourceWatcherService


class StubProvider:
    def __init__(self) -> None:
        self.calls = 0

    def supports(self, source_type: str) -> bool:
        return source_type == "rss"

    def check_for_update(self, subscription: SourceSubscription, state: CheckState):
        self.calls += 1
        if self.calls == 1:
            return (
                {
                    "update_id": "entry-2",
                    "title": "New entry",
                    "published_at": datetime(2024, 1, 1, tzinfo=timezone.utc),
                    "payload": {"link": "https://example.com/entry-2"},
                },
                CheckState(last_update_id="entry-2"),
            )
        return (None, CheckState(last_update_id="entry-2"))


class StubNotifier:
    def __init__(self) -> None:
        self.events = []

    def notify(self, subscription: SourceSubscription, update) -> None:
        self.events.append((subscription.source_id, update.update_id))


def test_source_watcher_notifies_only_when_new_update_detected() -> None:
    service = SourceWatcherService(providers=[StubProvider()], notifier=StubNotifier())
    subscription = SourceSubscription(
        source_id="source-1",
        source_type="rss",
        source_url="https://example.com/feed.xml",
        target_services=["https://consumer.local/hook"],
    )

    first = service.check_subscription(subscription)
    second = service.check_subscription(subscription)

    assert first is not None
    assert first.update_id == "entry-2"
    assert second is None
