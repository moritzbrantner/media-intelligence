from __future__ import annotations

from datetime import timezone
import json

from mi_discovery.source_watcher.models import CheckState, SourceSubscription
from mi_discovery.source_watcher.providers import RssProvider, ToolFeedProvider


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

    monkeypatch.setattr("mi_discovery.source_watcher.providers.requests.get", fake_get)

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


def test_tool_feed_provider_detects_social_channel_updates(monkeypatch) -> None:
    provider = ToolFeedProvider()
    subscription = SourceSubscription(
        source_id="yt-channel",
        source_type="youtube",
        source_url="https://www.youtube.com/@example/videos",
        target_services=["https://consumer.local/hook"],
    )

    def fake_load_feed(source_url: str):
        return {
            "title": "Example Channel",
            "entries": [
                {
                    "id": "video-123",
                    "title": "Latest upload",
                    "timestamp": 1708423200,
                    "webpage_url": "https://youtube.com/watch?v=video-123",
                    "url": "https://media.example/video-123.mp4",
                    "channel": "Example Channel",
                    "channel_id": "UC123",
                }
            ],
        }

    monkeypatch.setattr(provider, "_load_feed", fake_load_feed)

    update, state = provider.check_for_update(subscription, CheckState())
    assert update is not None
    assert update["update_id"] == "video-123"
    assert update["payload"]["platform"] == "youtube"
    assert update["payload"]["webpage_url"] == "https://youtube.com/watch?v=video-123"
    assert "download_url" not in update["payload"]
    assert update["published_at"].tzinfo == timezone.utc
    assert state.last_update_id == "video-123"

    update_again, next_state = provider.check_for_update(subscription, state)
    assert update_again is None
    assert next_state.last_update_id == "video-123"


def test_tool_feed_provider_passes_source_url_after_option_terminator(monkeypatch) -> None:
    captured: dict = {}

    class Completed:
        stdout = '{"entries": []}'

    def fake_run(args, **kwargs):
        captured["args"] = args
        captured["kwargs"] = kwargs
        return Completed()

    monkeypatch.setattr("mi_discovery.source_watcher.providers.subprocess.run", fake_run)

    feed = ToolFeedProvider()._load_feed("--exec=touch /tmp/pwned")

    assert feed == {"entries": []}
    assert captured["args"][-2:] == ["--", "--exec=touch /tmp/pwned"]
    assert captured["kwargs"]["timeout"] > 0


def test_tool_feed_provider_only_handles_youtube() -> None:
    provider = ToolFeedProvider()
    assert provider.supports("youtube")
    for source_type in ("twitter", "x", "facebook", "youtube_channel"):
        assert not provider.supports(source_type)


def test_tool_feed_provider_rejects_single_item_urls(monkeypatch) -> None:
    provider = ToolFeedProvider()
    subscription = SourceSubscription(
        source_id="single",
        source_type="youtube",
        source_url="https://www.youtube.com/watch?v=video-123",
        target_services=["https://consumer.local/hook"],
    )
    monkeypatch.setattr(provider, "_load_feed", lambda url: {"id": "video-123"})

    try:
        provider.check_for_update(subscription, CheckState())
    except ValueError as exc:
        assert "channel" in str(exc)
    else:
        raise AssertionError("expected ValueError for a single-item source_url")


def test_tool_feed_provider_falls_back_to_upload_date(monkeypatch) -> None:
    provider = ToolFeedProvider()
    subscription = SourceSubscription(
        source_id="yt",
        source_type="youtube",
        source_url="https://www.youtube.com/@example/videos",
        target_services=["https://consumer.local/hook"],
    )
    monkeypatch.setattr(
        provider,
        "_load_feed",
        lambda url: {"entries": [{"id": "v1", "upload_date": "20240220"}]},
    )

    update, _ = provider.check_for_update(subscription, CheckState())

    assert update is not None
    assert update["published_at"].isoformat() == "2024-02-20T00:00:00+00:00"


def test_tool_feed_provider_rejects_playlist_urls(monkeypatch) -> None:
    provider = ToolFeedProvider()
    monkeypatch.setattr(
        provider, "_load_feed", lambda url: (_ for _ in ()).throw(AssertionError("not called"))
    )
    for url in (
        "https://www.youtube.com/playlist?list=PL123",
        "https://www.youtube.com/watch?v=abc&list=PL123",
    ):
        subscription = SourceSubscription(
            source_id="pl",
            source_type="youtube",
            source_url=url,
            target_services=["https://consumer.local/hook"],
        )
        try:
            provider.check_for_update(subscription, CheckState())
        except ValueError as exc:
            assert "playlists" in str(exc)
        else:
            raise AssertionError(f"expected ValueError for {url}")


def test_channel_uploads_url_normalizes_root_channels() -> None:
    from mi_discovery.source_watcher.providers import _channel_uploads_url

    assert _channel_uploads_url("https://www.youtube.com/@example") == (
        "https://www.youtube.com/@example/videos"
    )
    assert _channel_uploads_url("https://www.youtube.com/channel/UC123/") == (
        "https://www.youtube.com/channel/UC123/videos"
    )
    assert _channel_uploads_url("https://youtube.com/@example/shorts?view=0") == (
        "https://youtube.com/@example/shorts"
    )
    for url in (
        "https://www.youtube.com/@example/streams",
        "https://www.youtube.com/@example/playlists",
        "https://www.youtube.com/@example/featured",
        "https://www.youtube.com/watch?v=abc",
        "https://example.com/@example",
    ):
        try:
            _channel_uploads_url(url)
        except ValueError:
            pass
        else:
            raise AssertionError(f"expected ValueError for {url}")


def test_tool_feed_provider_loads_the_uploads_tab(monkeypatch) -> None:
    provider = ToolFeedProvider()
    requested: list[str] = []

    def fake_load_feed(url: str):
        requested.append(url)
        return {"entries": [{"id": "v1"}]}

    monkeypatch.setattr(provider, "_load_feed", fake_load_feed)
    subscription = SourceSubscription(
        source_id="yt",
        source_type="youtube",
        source_url="https://www.youtube.com/@example",
        target_services=["https://consumer.local/hook"],
    )

    provider.check_for_update(subscription, CheckState())

    assert requested == ["https://www.youtube.com/@example/videos"]


def test_tool_feed_provider_reports_yt_dlp_failures_as_source_check_errors(monkeypatch) -> None:
    import subprocess

    from mi_discovery.source_watcher.providers import SourceCheckError

    provider = ToolFeedProvider()
    failures = [
        FileNotFoundError("yt-dlp"),
        subprocess.TimeoutExpired(["yt-dlp"], 120),
        subprocess.CalledProcessError(1, ["yt-dlp"], stderr="ERROR: unavailable\n"),
        json.JSONDecodeError("bad", "", 0),
    ]
    for failure in failures:
        def fail(url: str, failure=failure):
            raise failure

        monkeypatch.setattr(provider, "_run_yt_dlp", fail)
        try:
            provider._load_feed("https://www.youtube.com/@example/videos")
        except SourceCheckError:
            pass
        else:
            raise AssertionError(f"expected SourceCheckError for {failure!r}")
