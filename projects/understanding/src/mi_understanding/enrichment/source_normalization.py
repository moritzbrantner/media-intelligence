from __future__ import annotations

import hashlib
from typing import Any

from .content_models import ContentDocument


def normalize_source_payload(source_type: str, payload: dict[str, Any]) -> ContentDocument:
    """Normalize a source payload into the common ContentDocument contract."""
    normalizers: dict[str, Any] = {
        "website": _normalize_website_payload,
    }
    try:
        normalizer = normalizers[source_type]
    except KeyError as exc:
        raise ValueError(f"Unsupported source_type for normalization: {source_type}") from exc
    document = normalizer(payload)
    return ContentDocument.model_validate(document)


def _normalize_website_payload(payload: dict[str, Any]) -> dict[str, Any]:
    canonical_url = payload.get("canonical_url") or payload.get("url")
    if not canonical_url:
        raise ValueError("website payload requires `url` or `canonical_url`")

    raw_text = (payload.get("text") or "").strip()
    body_text = raw_text or None

    title = payload.get("title")
    description = payload.get("metadata", {}).get("description")

    sections: list[str] = []
    if title:
        sections.append(f"[TITLE]\n{title}")
    if body_text:
        sections.append(f"[BODY]\n{body_text}")
    if not sections:
        sections.append("[BODY]")

    return {
        "schema_version": "v1",
        "media_id": _website_media_id(canonical_url),
        "source_type": "website",
        "source_url": payload.get("url") or canonical_url,
        "title": title,
        "description": description,
        "published_at": payload.get("metadata", {}).get("published_at"),
        "language": payload.get("metadata", {}).get("lang"),
        "transcript_segments": [],
        "ocr_frames": [],
        "body_text": body_text,
        "full_text": "\n\n".join(sections),
        "source_metadata": {
            "url": payload.get("url"),
            "canonical_url": payload.get("canonical_url"),
            "links": payload.get("links", []),
            "images": payload.get("images", []),
            "audio": payload.get("audio", []),
            "metadata": payload.get("metadata", {}),
            "fetched_at": payload.get("fetched_at"),
        },
    }


def _website_media_id(canonical_url: str) -> str:
    digest = hashlib.sha256(canonical_url.encode("utf-8")).hexdigest()[:12]
    return f"website_{digest}"

