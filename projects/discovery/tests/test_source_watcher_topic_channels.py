# Independent acceptance tests for issue #22: YouTube channels without a Videos tab
# (Topic channels, yt-dlp/yt-dlp#10804). Written before the fix. yt-dlp is stubbed at
# the subprocess.run boundary and answers per requested URL, so the tests exercise the
# provider's real yt-dlp invocation and error handling without depending on private
# helper names.
from __future__ import annotations

import json
import subprocess
from urllib.parse import parse_qs, urlparse

import pytest

from mi_discovery.source_watcher.models import CheckState, SourceSubscription
from mi_discovery.source_watcher.providers import SourceCheckError, ToolFeedProvider

TOPIC_ID = "UCtopic0123456789abcdefg"
TOPIC_UPLOADS = "UU" + TOPIC_ID[2:]
NO_VIDEOS_TAB = f"ERROR: [youtube:tab] {TOPIC_ID}: This channel does not have a videos tab\n"


class Completed:
    def __init__(self, stdout: str = "", returncode: int = 0, stderr: str = "") -> None:
        self.stdout = stdout
        self.returncode = returncode
        self.stderr = stderr


def uploads_feed(*video_ids: str, channel_id: str = TOPIC_ID) -> str:
    return json.dumps(
        {
            "id": "UU" + channel_id[2:],
            "title": "Uploads from Example - Topic",
            "channel_id": channel_id,
            "entries": [
                {
                    "id": vid,
                    "title": f"Track {vid}",
                    "url": f"https://www.youtube.com/watch?v={vid}",
                    "webpage_url": f"https://www.youtube.com/watch?v={vid}",
                    "channel_id": channel_id,
                }
                for vid in video_ids
            ],
        }
    )


def channel_root_metadata(channel_id: str | None) -> str:
    data: dict = {"title": "Example - Topic", "entries": [{"_type": "url", "url": "x"}]}
    if channel_id is not None:
        data["id"] = channel_id
        data["channel_id"] = channel_id
    return json.dumps(data)


class UrlRoutedYtDlp:
    """Stands in for subprocess.run and answers by the URL yt-dlp was asked to load.

    ``routes`` maps a matcher key (see ``_key``) to a Completed result. URLs without a
    route fail like yt-dlp would, and are recorded so tests can assert on them.
    """

    def __init__(self, routes: dict[str, Completed]) -> None:
        self.routes = routes
        self.calls: list[list[str]] = []

    @staticmethod
    def _key(url: str) -> str:
        parsed = urlparse(url)
        if parsed.path.rstrip("/") == "/playlist":
            lists = parse_qs(parsed.query).get("list", [""])
            return "playlist:" + lists[0]
        return "path:" + parsed.path.rstrip("/")

    def urls(self) -> list[str]:
        return [call[-1] for call in self.calls]

    def keys(self) -> list[str]:
        return [self._key(url) for url in self.urls()]

    def __call__(self, args, **kwargs):
        args = list(args)
        self.calls.append(args)
        assert args[0] == "yt-dlp"
        # Option terminator must directly precede the URL (PR #14 hardening).
        assert len(args) >= 2 and args[-2] == "--", args
        result = self.routes.get(self._key(args[-1]))
        if result is None:
            result = Completed("", 1, f"ERROR: unexpected URL in test stub: {args[-1]}\n")
        if result.returncode and kwargs.get("check"):
            raise subprocess.CalledProcessError(
                result.returncode, args, output=result.stdout, stderr=result.stderr
            )
        return result


def subscription(url: str) -> SourceSubscription:
    return SourceSubscription(
        source_id="topic",
        source_type="youtube",
        source_url=url,
        target_services=["https://consumer.local/hook"],
    )


def install(monkeypatch, routes: dict[str, Completed]) -> UrlRoutedYtDlp:
    fake = UrlRoutedYtDlp(routes)
    monkeypatch.setattr(subprocess, "run", fake)
    return fake


@pytest.mark.parametrize(
    "url",
    [
        f"https://www.youtube.com/channel/{TOPIC_ID}",
        f"https://www.youtube.com/channel/{TOPIC_ID}/",
        f"https://www.youtube.com/channel/{TOPIC_ID}/videos",
    ],
    ids=["root", "root-slash", "explicit-videos"],
)
def test_topic_channel_id_url_falls_back_to_uploads_playlist(monkeypatch, url) -> None:
    fake = install(
        monkeypatch,
        {
            f"path:/channel/{TOPIC_ID}/videos": Completed("", 1, NO_VIDEOS_TAB),
            # Allowed but not required: the id is already in the URL.
            f"path:/channel/{TOPIC_ID}": Completed(channel_root_metadata(TOPIC_ID)),
            f"playlist:{TOPIC_UPLOADS}": Completed(uploads_feed("newest01", "older002")),
        },
    )

    update, state = ToolFeedProvider().check_for_update(subscription(url), CheckState())

    assert f"playlist:{TOPIC_UPLOADS}" in fake.keys()
    assert fake.keys()[-1] == f"playlist:{TOPIC_UPLOADS}"
    assert update is not None
    assert update["update_id"] == "newest01"
    assert update["payload"]["webpage_url"] == "https://www.youtube.com/watch?v=newest01"
    assert state.last_update_id == "newest01"


def test_topic_channel_handle_url_resolves_channel_id_and_reads_uploads(monkeypatch) -> None:
    # The handle does not reveal the channel id; it must come from yt-dlp (channel
    # metadata, or the error message that names the channel id).
    fake = install(
        monkeypatch,
        {
            "path:/@exampletopic/videos": Completed("", 1, NO_VIDEOS_TAB),
            "path:/@exampletopic": Completed(channel_root_metadata(TOPIC_ID)),
            f"playlist:{TOPIC_UPLOADS}": Completed(uploads_feed("newest01", "older002")),
        },
    )

    update, state = ToolFeedProvider().check_for_update(
        subscription("https://www.youtube.com/@exampletopic"), CheckState()
    )

    assert fake.keys()[0] == "path:/@exampletopic/videos"
    assert fake.keys()[-1] == f"playlist:{TOPIC_UPLOADS}"
    assert update is not None
    assert update["update_id"] == "newest01"
    assert update["payload"]["webpage_url"] == "https://www.youtube.com/watch?v=newest01"
    assert state.last_update_id == "newest01"


def test_topic_channel_fallback_reports_no_update_when_newest_unchanged(monkeypatch) -> None:
    install(
        monkeypatch,
        {
            f"path:/channel/{TOPIC_ID}/videos": Completed("", 1, NO_VIDEOS_TAB),
            f"path:/channel/{TOPIC_ID}": Completed(channel_root_metadata(TOPIC_ID)),
            f"playlist:{TOPIC_UPLOADS}": Completed(uploads_feed("newest01", "older002")),
        },
    )

    update, state = ToolFeedProvider().check_for_update(
        subscription(f"https://www.youtube.com/channel/{TOPIC_ID}"),
        CheckState(last_update_id="newest01"),
    )

    assert update is None
    assert state.last_update_id == "newest01"


@pytest.mark.parametrize(
    ("url", "videos_path"),
    [
        ("https://www.youtube.com/@example", "/@example/videos"),
        ("https://www.youtube.com/@example/videos", "/@example/videos"),
        ("https://www.youtube.com/channel/UC123/videos", "/channel/UC123/videos"),
    ],
)
def test_ordinary_channel_reads_videos_tab_with_single_call(monkeypatch, url, videos_path) -> None:
    fake = install(
        monkeypatch, {"path:" + videos_path: Completed(uploads_feed("vid-1", channel_id="UC123"))}
    )

    update, _ = ToolFeedProvider().check_for_update(subscription(url), CheckState())

    assert fake.keys() == ["path:" + videos_path]
    assert update is not None and update["update_id"] == "vid-1"


@pytest.mark.parametrize(
    "url",
    [
        "https://www.youtube.com/playlist?list=PL123",
        "https://www.youtube.com/watch?v=abc&list=PL123",
        "https://www.youtube.com/@example/streams",
        f"https://www.youtube.com/channel/{TOPIC_ID}/streams",
    ],
)
def test_playlists_and_streams_still_rejected_without_yt_dlp(monkeypatch, url) -> None:
    fake = install(monkeypatch, {})

    with pytest.raises(ValueError):
        ToolFeedProvider().check_for_update(subscription(url), CheckState())

    assert fake.calls == []


def test_other_yt_dlp_failure_is_reported_without_fallback(monkeypatch) -> None:
    fake = install(
        monkeypatch,
        {
            f"path:/channel/{TOPIC_ID}/videos": Completed(
                "", 1, "ERROR: [youtube:tab] Unable to download webpage: <urlopen error [Errno -3] "
                "Temporary failure in name resolution>\n"
            ),
            f"path:/channel/{TOPIC_ID}": Completed(channel_root_metadata(TOPIC_ID)),
            f"playlist:{TOPIC_UPLOADS}": Completed(uploads_feed("newest01")),
        },
    )

    with pytest.raises(SourceCheckError):
        ToolFeedProvider().check_for_update(
            subscription(f"https://www.youtube.com/channel/{TOPIC_ID}"), CheckState()
        )

    assert fake.keys() == [f"path:/channel/{TOPIC_ID}/videos"]


def test_shorts_tab_missing_is_not_redirected_to_uploads(monkeypatch) -> None:
    # Preflight scope: the fallback applies only to the (implicit or explicit) /videos tab.
    fake = install(
        monkeypatch,
        {
            f"path:/channel/{TOPIC_ID}/shorts": Completed(
                "", 1, f"ERROR: [youtube:tab] {TOPIC_ID}: This channel does not have a shorts tab\n"
            ),
            f"playlist:{TOPIC_UPLOADS}": Completed(uploads_feed("newest01")),
        },
    )

    with pytest.raises(SourceCheckError):
        ToolFeedProvider().check_for_update(
            subscription(f"https://www.youtube.com/channel/{TOPIC_ID}/shorts"), CheckState()
        )

    assert f"playlist:{TOPIC_UPLOADS}" not in fake.keys()


@pytest.mark.parametrize(
    "root_result",
    [
        Completed(channel_root_metadata(None)),
        Completed("", 1, "ERROR: [youtube:tab] Unable to download webpage: HTTP Error 503\n"),
    ],
    ids=["metadata-without-channel-id", "metadata-call-fails"],
)
def test_undeterminable_channel_id_fails_check(monkeypatch, root_result) -> None:
    # The error message names the handle, not a UC id, so the id cannot be read from it.
    fake = install(
        monkeypatch,
        {
            "path:/@nochannelid/videos": Completed(
                "", 1, "ERROR: [youtube:tab] @nochannelid: This channel does not have a videos tab\n"
            ),
            "path:/@nochannelid": root_result,
        },
    )

    with pytest.raises(SourceCheckError):
        ToolFeedProvider().check_for_update(
            subscription("https://www.youtube.com/@nochannelid"), CheckState()
        )

    assert not any(key.startswith("playlist:") for key in fake.keys())
