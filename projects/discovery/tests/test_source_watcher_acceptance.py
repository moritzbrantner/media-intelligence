# Retrospective acceptance (PR #14): tests derived from services/SOURCE_WATCHER.md
# and the PR description, written after the implementation landed. They drive the
# real POST /v1/check HTTP handler with yt-dlp mocked at the subprocess boundary
# (no network). Points already covered in test_source_watcher_providers.py are not
# repeated here.
from __future__ import annotations

from http.server import ThreadingHTTPServer
import json
import subprocess
import threading
import urllib.error
import urllib.request

import pytest

class RecordingNotifier:
    def __init__(self) -> None:
        self.events: list = []

    def notify(self, subscription, update) -> None:
        self.events.append(update)


class Completed:
    def __init__(self, stdout: str, returncode: int = 0, stderr: str = "") -> None:
        self.stdout = stdout
        self.returncode = returncode
        self.stderr = stderr


class FakeYtDlp:
    """Stands in for subprocess.run; records calls and returns a scripted result."""

    def __init__(self, result) -> None:
        self.result = result
        self.calls: list[list[str]] = []

    def __call__(self, args, **kwargs):
        self.calls.append(list(args))
        result = self.result
        if isinstance(result, BaseException):
            raise result
        if result.returncode and kwargs.get("check"):
            raise subprocess.CalledProcessError(
                result.returncode, args, output=result.stdout, stderr=result.stderr
            )
        return result


@pytest.fixture()
def server(monkeypatch):
    from mi_discovery.source_watcher import main

    notifier = RecordingNotifier()
    monkeypatch.setattr(main.SERVICE, "notifier", notifier)
    monkeypatch.setattr(main.SERVICE, "_state", {})
    httpd = ThreadingHTTPServer(("127.0.0.1", 0), main.Handler)
    thread = threading.Thread(target=httpd.serve_forever, daemon=True)
    thread.start()
    try:
        yield httpd, notifier
    finally:
        httpd.shutdown()
        httpd.server_close()


def post_check(httpd, source_url: str, source_id: str = "yt") -> tuple[int, dict]:
    body = json.dumps(
        {
            "source_id": source_id,
            "source_type": "youtube",
            "source_url": source_url,
            "target_services": ["http://consumer.local/hook"],
        }
    ).encode("utf-8")
    request = urllib.request.Request(
        f"http://127.0.0.1:{httpd.server_address[1]}/v1/check",
        data=body,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            return response.status, json.loads(response.read())
    except urllib.error.HTTPError as exc:
        return exc.code, json.loads(exc.read())


def channel_feed(entry_id: str, **entry_fields) -> str:
    entry = {
        "id": entry_id,
        "title": f"Upload {entry_id}",
        "webpage_url": f"https://www.youtube.com/watch?v={entry_id}",
        "url": f"https://www.youtube.com/watch?v={entry_id}",
        "channel": "Example Channel",
        "channel_id": "UC123",
        "uploader_id": "@example",
        **entry_fields,
    }
    return json.dumps({"id": "UC123", "title": "Example Channel - Videos", "entries": [entry]})


def test_update_detected_then_no_update_through_http(server, monkeypatch) -> None:
    httpd, notifier = server
    fake = FakeYtDlp(Completed(channel_feed("vid-1", timestamp=1708423200)))
    monkeypatch.setattr(subprocess, "run", fake)

    status, body = post_check(httpd, "https://www.youtube.com/@example")

    assert status == 200
    assert body["status"] == "update_detected"
    update = body["update"]
    assert update["update_id"] == "vid-1"
    assert update["published_at"].startswith("2024-02-20T10:00:00")
    payload = update["payload"]
    assert payload["platform"] == "youtube"
    assert payload["webpage_url"] == "https://www.youtube.com/watch?v=vid-1"
    assert "download_url" not in payload
    assert payload["channel_name"] == "Example Channel"
    assert payload["channel_id"] == "UC123"
    assert payload["uploader_id"] == "@example"
    assert payload["raw_entry"]["id"] == "vid-1"
    assert [event.update_id for event in notifier.events] == ["vid-1"]

    # yt-dlp is invoked with the documented flags on the normalized /videos tab.
    args = fake.calls[0]
    assert args[0].endswith("yt-dlp") or "yt_dlp" in args
    for flag in ("--dump-single-json", "--flat-playlist"):
        assert flag in args
    assert args[args.index("--playlist-end") + 1] == "1"
    assert args[args.index("--extractor-args") + 1] == "youtubetab:approximate_date"
    assert args[-2:] == ["--", "https://www.youtube.com/@example/videos"]

    # Same newest id on the next poll: no update, no second notification.
    status, body = post_check(httpd, "https://www.youtube.com/@example")
    assert (status, body) == (200, {"status": "no_update"})
    assert len(notifier.events) == 1

    # A new newest id is a new update.
    fake.result = Completed(channel_feed("vid-2", upload_date="20240221"))
    status, body = post_check(httpd, "https://www.youtube.com/@example")
    assert status == 200 and body["status"] == "update_detected"
    assert body["update"]["update_id"] == "vid-2"
    assert body["update"]["published_at"].startswith("2024-02-21T00:00:00")


@pytest.mark.parametrize(
    "url",
    [
        "https://www.youtube.com/@example/videos",
        "https://www.youtube.com/@example/shorts",
        "https://www.youtube.com/channel/UC123/videos",
    ],
)
def test_upload_tabs_are_accepted_as_is(server, monkeypatch, url) -> None:
    httpd, _ = server
    fake = FakeYtDlp(Completed(channel_feed("vid-1")))
    monkeypatch.setattr(subprocess, "run", fake)

    status, body = post_check(httpd, url)

    assert status == 200 and body["status"] == "update_detected"
    assert fake.calls[0][-1] == url


@pytest.mark.parametrize(
    "url",
    [
        "https://www.youtube.com/@example/streams",
        "https://www.youtube.com/@example/playlists",
        "https://www.youtube.com/@example/community",
        "https://www.youtube.com/playlist?list=PL123",
        "https://www.youtube.com/watch?v=abc&list=PL123",
        "https://www.youtube.com/watch?v=abc",
        "https://vimeo.com/example",
        "https://example.com/@example",
    ],
)
def test_unsupported_urls_rejected_with_400_before_yt_dlp(server, monkeypatch, url) -> None:
    httpd, notifier = server
    fake = FakeYtDlp(AssertionError("yt-dlp must not run"))
    monkeypatch.setattr(subprocess, "run", fake)

    status, body = post_check(httpd, url)

    assert status == 400
    assert body.get("error")
    assert fake.calls == []
    assert notifier.events == []


def test_single_item_result_rejected_with_400(server, monkeypatch) -> None:
    httpd, notifier = server
    single = json.dumps({"id": "vid-1", "title": "A single video"})
    monkeypatch.setattr(subprocess, "run", FakeYtDlp(Completed(single)))

    status, body = post_check(httpd, "https://www.youtube.com/@example/videos")

    assert status == 400
    assert body.get("error")
    assert notifier.events == []


@pytest.mark.parametrize(
    "result",
    [
        FileNotFoundError(2, "No such file or directory", "yt-dlp"),
        subprocess.TimeoutExpired(["yt-dlp"], 120),
        Completed("", returncode=1, stderr="ERROR: unavailable\n"),
        Completed("this is not json"),
    ],
    ids=["missing", "timeout", "nonzero-exit", "invalid-json"],
)
def test_yt_dlp_failures_answer_502(server, monkeypatch, result) -> None:
    httpd, notifier = server
    monkeypatch.setattr(subprocess, "run", FakeYtDlp(result))

    status, body = post_check(httpd, "https://www.youtube.com/@example/videos")

    assert status == 502
    assert body["error"] == "source_check_failed"
    assert "detail" in body
    assert notifier.events == []
