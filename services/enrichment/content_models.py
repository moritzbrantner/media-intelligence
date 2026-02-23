from __future__ import annotations

from pydantic import BaseModel, Field


class Author(BaseModel):
    id: str | None = None
    name: str | None = None
    handle: str | None = None
    profile_url: str | None = None


class Channel(BaseModel):
    id: str | None = None
    name: str | None = None
    url: str | None = None


class TranscriptSegment(BaseModel):
    media_id: str
    segment_index: int
    speaker_id: str
    speaker_name: str | None = None
    start_seconds: float
    end_seconds: float
    text: str
    language: str
    confidence: float


class OcrFrame(BaseModel):
    media_id: str
    frame_index: int
    timestamp_seconds: float
    scene_change: bool
    text_blocks: list[dict] = Field(default_factory=list)
    full_text: str
    storage_path: str | None = None


class ContentDocument(BaseModel):
    media_id: str
    source_type: str
    source_url: str | None = None
    title: str | None = None
    description: str | None = None
    author: Author = Field(default_factory=Author)
    channel: Channel = Field(default_factory=Channel)
    published_at: str | None = None
    language: str | None = None
    transcript_segments: list[TranscriptSegment] = Field(default_factory=list)
    ocr_frames: list[OcrFrame] = Field(default_factory=list)
    body_text: str | None = None
    full_text: str
    source_metadata: dict = Field(default_factory=dict)
