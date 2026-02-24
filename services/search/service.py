from __future__ import annotations

from typing import Any

from .elastic import ElasticsearchClient
from .models import SearchRequest


class SearchService:
    def __init__(self, es_client: ElasticsearchClient) -> None:
        self.es_client = es_client

    def search_moments(self, request: SearchRequest) -> dict[str, Any]:
        hits = self.es_client.search_segments(request)

        media_ids = [hit.get("_source", {}).get("media_id") for hit in hits]
        media_ids = [media_id for media_id in media_ids if media_id]

        documents = self.es_client.get_documents(list(dict.fromkeys(media_ids)))

        ranked_hits: list[dict[str, Any]] = []
        for hit in hits:
            source = hit.get("_source", {})
            media_id = source.get("media_id")
            document = documents.get(media_id, {})

            if not _matches_document_filters(request, document):
                continue

            start = float(source.get("start_seconds") or 0.0)
            end = float(source.get("end_seconds") or start)
            clip_start = max(0.0, start - 5.0)
            clip_end = max(clip_start, end + 5.0)

            ranked_hits.append(
                {
                    "media_id": media_id,
                    "segment_id": _segment_id(media_id, source),
                    "start_seconds": start,
                    "end_seconds": end,
                    "text": source.get("text", ""),
                    "score_breakdown": {
                        "total": hit.get("_score", 0.0),
                        "lexical": hit.get("_score", 0.0),
                        "semantic": hit.get("_score", 0.0) if request.fuzzy_semantic else 0.0,
                    },
                    "navigation": {
                        "clip_start": clip_start,
                        "clip_end": clip_end,
                        "canonical_media_url": document.get("source_url"),
                        "player_deep_link": _build_player_deep_link(media_id, start, end),
                    },
                }
            )

            if len(ranked_hits) >= request.top_k:
                break

        return {
            "query": request.query,
            "count": len(ranked_hits),
            "hits": ranked_hits,
        }


def _segment_id(media_id: str | None, source: dict[str, Any]) -> str:
    segment_index = source.get("provenance", {}).get("segment_index")
    if media_id and segment_index is not None:
        return f"{media_id}:seg:{segment_index}"
    if media_id:
        return f"{media_id}:seg"
    return "unknown:seg"


def _build_player_deep_link(media_id: str | None, start_seconds: float, end_seconds: float) -> dict[str, Any]:
    return {
        "media_id": media_id,
        "start_seconds": start_seconds,
        "end_seconds": end_seconds,
    }


def _matches_document_filters(request: SearchRequest, document: dict[str, Any]) -> bool:
    if request.filters.channel:
        channel_name = str(document.get("channel_name", "")).strip()
        if channel_name.lower() != request.filters.channel.lower():
            return False
    if request.filters.author:
        author_values = [
            str(document.get("author_name", "")).strip(),
            str(document.get("author_handle", "")).strip(),
        ]
        target = request.filters.author.lower()
        if target not in [value.lower() for value in author_values if value]:
            return False
    return True
