#!/usr/bin/env python3
"""HTTP entrypoint for the search service."""

from __future__ import annotations

from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os

from .elastic import ElasticsearchClient
from .models import parse_search_request
from .service import SearchService

HOST = os.getenv("SEARCH_SERVICE_HOST", "0.0.0.0")
PORT = int(os.getenv("SEARCH_SERVICE_PORT", "8088"))
ES_URL = os.getenv("ELASTICSEARCH_URL", "http://localhost:9200")

SERVICE = SearchService(es_client=ElasticsearchClient(base_url=ES_URL))


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
            self._write_json({"service": "search", "status": "ok"})
            return
        self._write_json({"error": "not_found"}, status=HTTPStatus.NOT_FOUND)

    def do_POST(self) -> None:  # noqa: N802
        if self.path != "/v1/search/moments":
            self._write_json({"error": "not_found"}, status=HTTPStatus.NOT_FOUND)
            return

        content_length = int(self.headers.get("Content-Length", "0"))
        raw_payload = self.rfile.read(content_length)
        payload = json.loads(raw_payload.decode("utf-8") or "{}")

        try:
            request = parse_search_request(payload)
            result = SERVICE.search_moments(request)
        except ValueError as exc:
            self._write_json({"error": str(exc)}, status=HTTPStatus.BAD_REQUEST)
            return

        self._write_json(result)


def main() -> None:
    server = ThreadingHTTPServer((HOST, PORT), Handler)
    print(f"search listening on {HOST}:{PORT}")
    server.serve_forever()


if __name__ == "__main__":
    main()

