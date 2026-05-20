from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass(slots=True)
class SearchFilters:
    speaker: str | None = None
    source_type: str | None = None
    date_from: str | None = None
    date_to: str | None = None
    channel: str | None = None
    author: str | None = None


@dataclass(slots=True)
class SearchRequest:
    query: str
    fuzzy_spelling: bool = True
    fuzzy_semantic: bool = True
    filters: SearchFilters = field(default_factory=SearchFilters)
    top_k: int = 20


def parse_search_request(payload: dict[str, Any]) -> SearchRequest:
    query = str(payload.get("query", "")).strip()
    if not query:
        raise ValueError("query is required")

    raw_filters = payload.get("filters") or {}
    if not isinstance(raw_filters, dict):
        raise ValueError("filters must be an object")

    filters = SearchFilters(
        speaker=_optional_text(raw_filters.get("speaker")),
        source_type=_optional_text(raw_filters.get("source_type")),
        date_from=_optional_text(raw_filters.get("date_from")),
        date_to=_optional_text(raw_filters.get("date_to")),
        channel=_optional_text(raw_filters.get("channel")),
        author=_optional_text(raw_filters.get("author")),
    )

    top_k = int(payload.get("top_k", 20))
    if top_k <= 0:
        raise ValueError("top_k must be greater than 0")

    return SearchRequest(
        query=query,
        fuzzy_spelling=bool(payload.get("fuzzy_spelling", True)),
        fuzzy_semantic=bool(payload.get("fuzzy_semantic", True)),
        filters=filters,
        top_k=min(top_k, 200),
    )


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None

