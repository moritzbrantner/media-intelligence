from __future__ import annotations

import json
from pathlib import Path

from mi_contracts import ContentDocumentV1, EnrichedDocumentV1, EventEnvelopeV1, MediaObjectV1


FIXTURES = Path(__file__).resolve().parents[1] / "fixtures"


def _load(name: str) -> dict:
    return json.loads((FIXTURES / name).read_text(encoding="utf-8"))


def test_media_object_fixture_validates() -> None:
    model = MediaObjectV1.model_validate(_load("media_object_v1.json"))
    assert model.schema_version == "v1"
    assert model.media_files[0].type == "text"


def test_content_document_fixture_validates() -> None:
    model = ContentDocumentV1.model_validate(_load("content_document_v1.json"))
    assert model.schema_version == "v1"
    assert model.body_text == "Main cleaned text"


def test_enriched_document_fixture_validates() -> None:
    model = EnrichedDocumentV1.model_validate(_load("enriched_document_v1.json"))
    assert model.topics[0].label == "policy"
    assert model.sentiment.document_level is not None


def test_event_envelope_fixture_validates() -> None:
    model = EventEnvelopeV1.model_validate(_load("event_envelope_v1.json"))
    assert model.metadata.source_type == "website"
