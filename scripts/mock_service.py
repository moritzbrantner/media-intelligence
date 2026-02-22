#!/usr/bin/env python3
"""Tiny mock service for local development and testing."""

from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from urllib.parse import parse_qs, urlparse

HOST = os.getenv("MOCK_SERVICE_HOST", "0.0.0.0")
PORT = int(os.getenv("MOCK_SERVICE_PORT", "8080"))


class MockHandler(BaseHTTPRequestHandler):
    service_name = "mock-service"

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
                    "job_id": f"mock-job-{idx}",
                    "state": "queued",
                    "source": "youtube",
                }
                for idx in range(1, max(1, min(limit, 25)) + 1)
            ]
            self._write_json({"jobs": jobs})
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
