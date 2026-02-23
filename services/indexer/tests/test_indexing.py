from __future__ import annotations

from pathlib import Path

from services.indexer.indexing import ChunkConfig, build_document_index_action, build_segment_index_actions, ensure_required_indices


class _FakeIndices:
    def __init__(self, existing: set[str] | None = None) -> None:
        self._existing = existing or set()
        self.created: list[tuple[str, dict]] = []

    def exists(self, index: str) -> bool:
        return index in self._existing

    def create(self, index: str, body: dict) -> None:
        self.created.append((index, body))
        self._existing.add(index)


class _FakeClient:
    def __init__(self, existing: set[str] | None = None) -> None:
        self.indices = _FakeIndices(existing)


def _enriched_payload() -> dict:
    return {
        "media_id": "website_abc123",
        "source_type": "website",
        "source_url": "https://example.com/post",
        "title": "Example post",
        "author": {"name": "Author"},
        "channel": {"name": "Channel"},
        "published_at": "2026-01-01T00:00:00Z",
        "language": "en",
        "topics": [{"label": "policy"}],
        "entities": [{"text": "OpenAI", "label": "ORG"}],
        "sentiment": {"document_level": {"label": "neutral", "score": 0.1}},
        "transcript_segments": [
            {
                "segment_index": 0,
                "speaker_id": "SPEAKER_00",
                "speaker_name": "Narrator",
                "start_seconds": 0.0,
                "end_seconds": 2.0,
                "text": "Transcript text",
            }
        ],
        "ocr_frames": [
            {"frame_index": 3, "timestamp_seconds": 2.5, "full_text": "Frame text"},
            {"frame_index": 4, "timestamp_seconds": 4.0, "full_text": "  "},
        ],
        "body_text": "Section one intro. Section one details. Section two summary.",
    }


def test_ensure_required_indices_creates_only_media_document_and_segment_indices() -> None:
    fake_client = _FakeClient(existing={"entity_profiles"})

    ensure_required_indices(fake_client, mappings_dir=Path("services/indexer/mappings"))

    created_indices = [idx for idx, _ in fake_client.indices.created]
    assert created_indices == ["media_documents", "media_segments"]


def test_build_document_index_action_uses_deterministic_media_id() -> None:
    action = build_document_index_action(_enriched_payload())

    assert action["_index"] == "media_documents"
    assert action["_id"] == "website_abc123"
    assert action["_source"]["source_url"] == "https://example.com/post"
    assert action["_source"]["topics"] == ["policy"]


def test_build_segment_index_actions_include_deterministic_ids_and_provenance() -> None:
    actions = build_segment_index_actions(_enriched_payload(), chunk_config=ChunkConfig(token_window=4, overlap=1))

    action_ids = [action["_id"] for action in actions]
    assert action_ids[0] == "website_abc123_seg_0"
    assert action_ids[1] == "website_abc123_seg_ocr_3"
    assert all(identifier.startswith("website_abc123_seg_") for identifier in action_ids)

    body_chunks = [action for action in actions if action["_source"]["segment_type"] == "body_text"]
    assert len(body_chunks) >= 1
    first_chunk = body_chunks[0]["_source"]
    assert first_chunk["provenance"]["origin"] == "body_text"
    assert isinstance(first_chunk["provenance"]["char_start"], int)
    assert isinstance(first_chunk["provenance"]["char_end"], int)
    assert first_chunk["provenance"]["char_end"] > first_chunk["provenance"]["char_start"]
