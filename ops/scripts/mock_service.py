#!/usr/bin/env python3
"""Tiny reusable mock service for local development and testing."""

from __future__ import annotations

from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from urllib.parse import parse_qs, urlparse

HOST = os.getenv("MOCK_SERVICE_HOST", "0.0.0.0")
PORT = int(os.getenv("MOCK_SERVICE_PORT", "8080"))
SERVICE_NAME = os.getenv("SERVICE_NAME", os.getenv("MOCK_SERVICE_NAME", "mock-service"))
SOURCE_NAME = os.getenv("MOCK_SOURCE", SERVICE_NAME.replace("mock-", ""))


class MockHandler(BaseHTTPRequestHandler):
    service_name = SERVICE_NAME
    source_name = SOURCE_NAME

    def _write_json(self, payload: dict, status: HTTPStatus = HTTPStatus.OK) -> None:
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:  # noqa: N802
        parsed = urlparse(self.path)

        if parsed.path in {"/", "/health"}:
            self._write_json(
                {
                    "service": self.service_name,
                    "status": "ok",
                    "mode": "mock",
                }
            )
            return

        if parsed.path == "/v1/jobs":
            query = parse_qs(parsed.query)
            limit = int(query.get("limit", ["3"])[0])
            jobs = [
                {
                    "job_id": f"{self.service_name}-job-{idx}",
                    "state": "queued",
                    "source": self.source_name,
                }
                for idx in range(1, max(1, min(limit, 25)) + 1)
            ]
            self._write_json({"jobs": jobs})
            return

        if parsed.path == "/v1/events":
            self._write_json(
                {
                    "service": self.service_name,
                    "events": [
                        {"event_type": "mock.started", "status": "ok"},
                        {"event_type": "mock.heartbeat", "status": "ok"},
                    ],
                }
            )
            return

        self._write_json(
            {
                "error": "not_found",
                "message": f"No mock route for path '{parsed.path}'",
            },
            status=HTTPStatus.NOT_FOUND,
        )


def main() -> None:
    server = ThreadingHTTPServer((HOST, PORT), MockHandler)
    print(f"{MockHandler.service_name} listening on {HOST}:{PORT}")
    server.serve_forever()


if __name__ == "__main__":
    main()

