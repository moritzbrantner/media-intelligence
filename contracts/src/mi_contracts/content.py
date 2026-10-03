from __future__ import annotations

from pydantic import BaseModel, Field


class AuthorV1(BaseModel):
    id: str | None = None
    name: str | None = None
    handle: str | None = None
    profile_url: str | None = None


class ChannelV1(BaseModel):
    id: str | None = None
    name: str | None = None
    url: str | None = None


class TranscriptSegmentV1(BaseModel):
    media_id: str
    segment_index: int
    speaker_id: str
    speaker_name: str | None = None
    start_seconds: float
    end_seconds: float
    text: str
    language: str
    confidence: float


class TextBlockV1(BaseModel):
    text: str
    confidence: float
    bounding_box: dict[str, float] = Field(default_factory=dict)


class OcrFrameV1(BaseModel):
    media_id: str
    frame_index: int
    timestamp_seconds: float
    scene_change: bool
    text_blocks: list[TextBlockV1] = Field(default_factory=list)
    full_text: str
    storage_path: str | None = None


class ContentDocumentV1(BaseModel):
    schema_version: str = "v1"
    media_id: str
    source_type: str
    source_url: str | None = None
    title: str | None = None
    description: str | None = None
    author: AuthorV1 = Field(default_factory=AuthorV1)
    channel: ChannelV1 = Field(default_factory=ChannelV1)
    published_at: str | None = None
    language: str | None = None
    transcript_segments: list[TranscriptSegmentV1] = Field(default_factory=list)
    ocr_frames: list[OcrFrameV1] = Field(default_factory=list)
    body_text: str | None = None
    full_text: str
    source_metadata: dict = Field(default_factory=dict)

