from __future__ import annotations

from typing import Any

import requests

from .models import SearchRequest


class ElasticsearchClient:
    def __init__(
        self,
        base_url: str,
        session: requests.Session | None = None,
        segments_index: str = "media_segments",
        documents_index: str = "media_documents",
    ) -> None:
        self.base_url = base_url.rstrip("/")
        self.session = session or requests.Session()
        self.segments_index = segments_index
        self.documents_index = documents_index

    def search_segments(self, request: SearchRequest) -> list[dict[str, Any]]:
        query_body = _build_segment_query(request)
        response = self.session.post(
            f"{self.base_url}/{self.segments_index}/_search",
            json=query_body,
            timeout=10,
        )
        response.raise_for_status()
        payload = response.json()
        return payload.get("hits", {}).get("hits", [])

    def get_documents(self, media_ids: list[str]) -> dict[str, dict[str, Any]]:
        if not media_ids:
            return {}

        response = self.session.post(
            f"{self.base_url}/{self.documents_index}/_search",
            json={
                "size": len(media_ids),
                "query": {"terms": {"media_id": media_ids}},
                "_source": [
                    "media_id",
                    "source_url",
                    "channel_name",
                    "author_name",
                    "author_handle",
                    "title",
                ],
            },
            timeout=10,
        )
        response.raise_for_status()
        hits = response.json().get("hits", {}).get("hits", [])
        return {hit.get("_source", {}).get("media_id"): hit.get("_source", {}) for hit in hits}


def _build_segment_query(request: SearchRequest) -> dict[str, Any]:
    must: list[dict[str, Any]] = []
    filters: list[dict[str, Any]] = []

    text_query: dict[str, Any] = {
        "multi_match": {
            "query": request.query,
            "fields": ["text^3", "topic", "stance"],
            "type": "best_fields",
        }
    }

    if request.fuzzy_spelling:
        text_query["multi_match"]["fuzziness"] = "AUTO"

    if request.fuzzy_semantic:
        text_query["multi_match"]["type"] = "most_fields"
        text_query["multi_match"]["minimum_should_match"] = "70%"

    must.append(text_query)

    if request.filters.speaker:
        filters.append({"term": {"speaker_name": request.filters.speaker}})
    if request.filters.source_type:
        filters.append({"term": {"source_type": request.filters.source_type}})
    if request.filters.date_from or request.filters.date_to:
        range_payload: dict[str, Any] = {}
        if request.filters.date_from:
            range_payload["gte"] = request.filters.date_from
        if request.filters.date_to:
            range_payload["lte"] = request.filters.date_to
        filters.append({"range": {"published_at": range_payload}})

    body: dict[str, Any] = {
        "size": request.top_k,
        "query": {
            "bool": {
                "must": must,
                "filter": filters,
            }
        },
        "_source": [
            "media_id",
            "start_seconds",
            "end_seconds",
            "text",
            "speaker_name",
            "source_type",
            "published_at",
            "provenance.segment_index",
        ],
    }
    return body
