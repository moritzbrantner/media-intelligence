#!/usr/bin/env python3
"""HTTP entrypoint for the source watcher service."""

from __future__ import annotations

from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os

from .notifier import WebhookNotifier
from .providers import RssProvider, ToolFeedProvider, WebsiteProvider, YouTubeProvider
from .service import SourceWatcherService, parse_subscription, serialize_update

HOST = os.getenv("SOURCE_WATCHER_HOST", "0.0.0.0")
PORT = int(os.getenv("SOURCE_WATCHER_PORT", "8087"))

SERVICE = SourceWatcherService(
    providers=[ToolFeedProvider(), YouTubeProvider(), RssProvider(), WebsiteProvider()],
    notifier=WebhookNotifier(),
)


class Handler(BaseHTTPRequestHandler):
    def _write_json(self, payload: dict, status: HTTPStatus = HTTPStatus.OK) -> None:
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:  # noqa: N802
        if self.path in {"/", "/health"}:
            self._write_json({"service": "source-watcher", "status": "ok"})
            return

        if self.path == "/v1/state":
            self._write_json({"state": SERVICE.list_states()})
            return

        self._write_json({"error": "not_found"}, status=HTTPStatus.NOT_FOUND)

    def do_POST(self) -> None:  # noqa: N802
        if self.path != "/v1/check":
            self._write_json({"error": "not_found"}, status=HTTPStatus.NOT_FOUND)
            return

        content_length = int(self.headers.get("Content-Length", "0"))
        raw_payload = self.rfile.read(content_length)
        payload = json.loads(raw_payload.decode("utf-8"))

        try:
            subscription = parse_subscription(payload)
            update = SERVICE.check_subscription(subscription)
        except ValueError as exc:
            self._write_json({"error": str(exc)}, status=HTTPStatus.BAD_REQUEST)
            return

        if update is None:
            self._write_json({"status": "no_update"})
            return

        self._write_json({"status": "update_detected", "update": serialize_update(update)})


def main() -> None:
    server = ThreadingHTTPServer((HOST, PORT), Handler)
    print(f"source-watcher listening on {HOST}:{PORT}")
    server.serve_forever()


if __name__ == "__main__":
    main()

