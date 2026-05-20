from __future__ import annotations

import hashlib
import json
from pathlib import Path

import pytest

from mi_understanding.enrichment.content_models import ContentDocument
from mi_understanding.enrichment.source_normalization import normalize_source_payload


FIXTURE_DIR = Path(__file__).parent / "fixtures" / "web_payloads"


def _fixture(name: str) -> dict:
    return json.loads((FIXTURE_DIR / name).read_text())


def test_normalize_website_payload_maps_into_content_document_contract() -> None:
    payload = {
        "url": "https://example.com/post",
        "canonical_url": "https://example.com/post",
        "title": "Example Post",
        "text": "Main cleaned text",
        "links": [{"url": "https://example.com/about", "anchor": "About", "rel": "nofollow"}],
        "images": [{"url": "https://example.com/image.jpg", "alt": "hero"}],
        "audio": [{"url": "https://example.com/audio.mp3", "type": "audio/mpeg", "source": "audio_tag"}],
        "metadata": {"lang": "en", "published_at": "2026-01-01T00:00:00Z"},
        "fetched_at": "2026-01-01T00:00:00Z",
    }

    doc = normalize_source_payload("website", payload)

    expected_hash = hashlib.sha256(payload["canonical_url"].encode("utf-8")).hexdigest()[:12]
    assert isinstance(doc, ContentDocument)
    assert doc.media_id == f"website_{expected_hash}"
    assert doc.source_type == "website"
    assert doc.body_text == "Main cleaned text"
    assert doc.transcript_segments == []
    assert doc.ocr_frames == []
    assert doc.source_metadata["links"] == payload["links"]
    assert doc.source_metadata["images"] == payload["images"]
    assert doc.source_metadata["audio"] == payload["audio"]


@pytest.mark.parametrize(
    "fixture_name,expected_media_seed,expected_body_text,links_count,audio_count",
    [
        ("missing_canonical_url.json", "https://example.com/post-without-canonical", "This page has only the URL field set.", 0, 0),
        ("empty_text.json", "https://example.com/empty", None, 0, 0),
        ("link_only_page.json", "https://example.com/links", None, 2, 0),
        ("audio_only_page.json", "https://example.com/audio", None, 0, 1),
    ],
)
def test_web_fixture_edge_cases_validate_with_content_document(
    fixture_name: str,
    expected_media_seed: str,
    expected_body_text: str | None,
    links_count: int,
    audio_count: int,
) -> None:
    payload = _fixture(fixture_name)

    doc = normalize_source_payload("website", payload)

    expected_hash = hashlib.sha256(expected_media_seed.encode("utf-8")).hexdigest()[:12]
    assert doc.media_id == f"website_{expected_hash}"
    assert doc.body_text == expected_body_text
    assert len(doc.source_metadata["links"]) == links_count
    assert len(doc.source_metadata["audio"]) == audio_count
    assert isinstance(ContentDocument.model_validate(doc.model_dump()), ContentDocument)
