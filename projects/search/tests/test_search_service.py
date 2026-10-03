from __future__ import annotations

from mi_search.models import parse_search_request
from mi_search.service import SearchService


class StubElasticsearchClient:
    def search_segments(self, request):
        return [
            {
                "_score": 9.5,
                "_source": {
                    "media_id": "m1",
                    "start_seconds": 12.0,
                    "end_seconds": 24.0,
                    "text": "Moment one",
                    "provenance": {"segment_index": 2},
                },
            },
            {
                "_score": 8.0,
                "_source": {
                    "media_id": "m2",
                    "start_seconds": 2.0,
                    "end_seconds": 4.0,
                    "text": "Moment two",
                },
            },
        ]

    def get_documents(self, media_ids):
        return {
            "m1": {
                "media_id": "m1",
                "source_url": "https://media.local/m1",
                "channel_name": "Channel One",
                "author_name": "Alice",
                "author_handle": "@alice",
            },
            "m2": {
                "media_id": "m2",
                "source_url": "https://media.local/m2",
                "channel_name": "Channel Two",
                "author_name": "Bob",
                "author_handle": "@bob",
            },
        }


def test_search_moments_shapes_ranked_hits_and_navigation_payload() -> None:
    service = SearchService(es_client=StubElasticsearchClient())
    request = parse_search_request({"query": "economy"})

    result = service.search_moments(request)

    assert result["count"] == 2
    first = result["hits"][0]
    assert first["media_id"] == "m1"
    assert first["segment_id"] == "m1:seg:2"
    assert first["navigation"]["clip_start"] == 7.0
    assert first["navigation"]["clip_end"] == 29.0
    assert first["navigation"]["canonical_media_url"] == "https://media.local/m1"


def test_search_moments_applies_channel_filter_from_media_documents() -> None:
    service = SearchService(es_client=StubElasticsearchClient())
    request = parse_search_request(
        {
            "query": "economy",
            "filters": {"channel": "Channel One"},
            "top_k": 10,
        }
    )

    result = service.search_moments(request)

    assert result["count"] == 1
    assert result["hits"][0]["media_id"] == "m1"
