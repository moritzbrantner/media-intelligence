from __future__ import annotations

from datetime import timezone

from services.source_watcher.models import CheckState, SourceSubscription
from services.source_watcher.providers import RssProvider


class StubResponse:
    def __init__(self, text: str) -> None:
        self.text = text

    def raise_for_status(self) -> None:
        return


def test_rss_provider_detects_new_item(monkeypatch) -> None:
    xml = """\
    <rss><channel><item>
      <guid>abc</guid>
      <title>Hello</title>
      <link>https://example.com/a</link>
      <pubDate>Tue, 20 Feb 2024 10:00:00 GMT</pubDate>
    </item></channel></rss>
    """

    def fake_get(url: str, timeout: int):
        return StubResponse(xml)

    monkeypatch.setattr("services.source_watcher.providers.requests.get", fake_get)

    provider = RssProvider()
    subscription = SourceSubscription(
        source_id="s1",
        source_type="rss",
        source_url="https://example.com/feed",
        target_services=["https://consumer.local/hook"],
    )

    update, next_state = provider.check_for_update(subscription, CheckState())

    assert update is not None
    assert update["update_id"] == "abc"
    assert update["published_at"].tzinfo == timezone.utc
    assert next_state.last_update_id == "abc"
