from __future__ import annotations

from mi_search.elastic import _build_segment_query
from mi_search.models import parse_search_request


def test_build_segment_query_includes_required_fields_and_filters() -> None:
    request = parse_search_request(
        {
            "query": "climate policy",
            "filters": {
                "speaker": "Jane",
                "source_type": "video",
                "date_from": "2024-01-01",
                "date_to": "2024-06-30",
            },
            "top_k": 15,
        }
    )

    body = _build_segment_query(request)

    assert body["size"] == 15
    assert body["query"]["bool"]["must"][0]["multi_match"]["fuzziness"] == "AUTO"
    filters = body["query"]["bool"]["filter"]
    assert {"term": {"speaker_name": "Jane"}} in filters
    assert {"term": {"source_type": "video"}} in filters
    assert {"range": {"published_at": {"gte": "2024-01-01", "lte": "2024-06-30"}}} in filters

