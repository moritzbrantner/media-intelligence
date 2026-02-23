from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
import json
from pathlib import Path
import re
from typing import Any


REQUIRED_INDICES = ("media_documents", "media_segments")
DEFAULT_SEGMENT_TOKEN_WINDOW = 180
DEFAULT_SEGMENT_TOKEN_OVERLAP = 30


@dataclass(frozen=True)
class ChunkConfig:
    token_window: int = DEFAULT_SEGMENT_TOKEN_WINDOW
    overlap: int = DEFAULT_SEGMENT_TOKEN_OVERLAP


def ensure_required_indices(es_client: Any, mappings_dir: Path | None = None) -> None:
    """Create required Elasticsearch indices when missing."""
    base_dir = mappings_dir or Path(__file__).resolve().parent / "mappings"

    for index_name in REQUIRED_INDICES:
        if es_client.indices.exists(index=index_name):
            continue

        mapping_path = base_dir / f"{index_name}.json"
        mapping = json.loads(mapping_path.read_text())
        es_client.indices.create(index=index_name, body=mapping)


def build_document_record(enriched: dict[str, Any]) -> dict[str, Any]:
    sentiment = enriched.get("sentiment", {}).get("document_level", {})

    return {
        "media_id": enriched["media_id"],
        "source_type": enriched["source_type"],
        "source_url": enriched.get("source_url"),
        "title": enriched.get("title"),
        "description": enriched.get("description"),
        "author_name": enriched.get("author", {}).get("name"),
        "author_handle": enriched.get("author", {}).get("handle"),
        "channel_name": enriched.get("channel", {}).get("name"),
        "published_at": enriched.get("published_at"),
        "language": enriched.get("language"),
        "topics": [topic["label"] for topic in enriched.get("topics", []) if topic.get("label")],
        "entities": [
            {"text": entity.get("text"), "label": entity.get("label")}
            for entity in enriched.get("entities", [])
            if entity.get("text") and entity.get("label")
        ],
        "sentiment_label": sentiment.get("label"),
        "sentiment_score": sentiment.get("score"),
        "indexed_at": datetime.now(timezone.utc).isoformat(),
    }


def build_document_index_action(enriched: dict[str, Any]) -> dict[str, Any]:
    return {
        "_op_type": "index",
        "_index": "media_documents",
        "_id": enriched["media_id"],
        "_source": build_document_record(enriched),
    }


def build_segment_index_actions(enriched: dict[str, Any], chunk_config: ChunkConfig | None = None) -> list[dict[str, Any]]:
    media_id = enriched["media_id"]
    source_type = enriched.get("source_type")
    published_at = enriched.get("published_at")

    actions: list[dict[str, Any]] = []

    for segment in enriched.get("transcript_segments", []):
        segment_index = segment["segment_index"]
        actions.append(
            {
                "_op_type": "index",
                "_index": "media_segments",
                "_id": f"{media_id}_seg_{segment_index}",
                "_source": {
                    "media_id": media_id,
                    "segment_type": "transcript",
                    "source_type": source_type,
                    "published_at": published_at,
                    "speaker_id": segment.get("speaker_id"),
                    "speaker_name": segment.get("speaker_name"),
                    "start_seconds": segment.get("start_seconds"),
                    "end_seconds": segment.get("end_seconds"),
                    "text": segment.get("text", ""),
                    "provenance": {
                        "origin": "transcript_segments",
                        "segment_index": segment_index,
                    },
                },
            }
        )

    for frame in enriched.get("ocr_frames", []):
        text = (frame.get("full_text") or "").strip()
        if not text:
            continue

        frame_index = frame["frame_index"]
        actions.append(
            {
                "_op_type": "index",
                "_index": "media_segments",
                "_id": f"{media_id}_seg_ocr_{frame_index}",
                "_source": {
                    "media_id": media_id,
                    "segment_type": "ocr",
                    "source_type": source_type,
                    "published_at": published_at,
                    "timestamp_seconds": frame.get("timestamp_seconds"),
                    "text": text,
                    "provenance": {
                        "origin": "ocr_frames",
                        "frame_index": frame_index,
                    },
                },
            }
        )

    body_text = (enriched.get("body_text") or "").strip()
    if body_text:
        actions.extend(
            _build_body_text_chunk_actions(
                media_id=media_id,
                source_type=source_type,
                published_at=published_at,
                body_text=body_text,
                chunk_config=chunk_config or ChunkConfig(),
            )
        )

    return actions


def _build_body_text_chunk_actions(
    *,
    media_id: str,
    source_type: str | None,
    published_at: str | None,
    body_text: str,
    chunk_config: ChunkConfig,
) -> list[dict[str, Any]]:
    chunks = _chunk_text_with_offsets(body_text, chunk_config.token_window, chunk_config.overlap)

    actions: list[dict[str, Any]] = []
    for chunk_number, chunk in enumerate(chunks):
        actions.append(
            {
                "_op_type": "index",
                "_index": "media_segments",
                "_id": f"{media_id}_seg_body_{chunk_number}",
                "_source": {
                    "media_id": media_id,
                    "segment_type": "body_text",
                    "source_type": source_type,
                    "published_at": published_at,
                    "text": chunk["text"],
                    "provenance": {
                        "origin": "body_text",
                        "chunk_index": chunk_number,
                        "char_start": chunk["char_start"],
                        "char_end": chunk["char_end"],
                    },
                },
            }
        )
    return actions


def _chunk_text_with_offsets(text: str, token_window: int, overlap: int) -> list[dict[str, Any]]:
    if token_window <= 0:
        raise ValueError("token_window must be > 0")
    if overlap < 0:
        raise ValueError("overlap must be >= 0")
    if overlap >= token_window:
        raise ValueError("overlap must be smaller than token_window")

    token_matches = list(re.finditer(r"\S+", text))
    if not token_matches:
        return []

    step = token_window - overlap
    chunks: list[dict[str, Any]] = []

    for start_idx in range(0, len(token_matches), step):
        end_idx = min(start_idx + token_window, len(token_matches))
        chunk_tokens = token_matches[start_idx:end_idx]
        if not chunk_tokens:
            continue

        char_start = chunk_tokens[0].start()
        char_end = chunk_tokens[-1].end()
        chunk_text = text[char_start:char_end]

        chunks.append(
            {
                "text": chunk_text,
                "char_start": char_start,
                "char_end": char_end,
            }
        )

        if end_idx == len(token_matches):
            break

    return chunks
