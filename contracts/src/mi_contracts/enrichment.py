from __future__ import annotations

from pydantic import BaseModel, Field

from .content import AuthorV1, ChannelV1, OcrFrameV1, TranscriptSegmentV1


class EntityV1(BaseModel):
    text: str
    label: str
    start_char: int
    end_char: int
    source: str


class KeywordV1(BaseModel):
    phrase: str
    score: float
    source: str


class TopicScoreV1(BaseModel):
    label: str
    score: float


class DocumentSentimentV1(BaseModel):
    label: str
    score: float


class SegmentSentimentV1(BaseModel):
    segment_index: int
    label: str
    score: float


class SentimentBundleV1(BaseModel):
    document_level: DocumentSentimentV1 | None = None
    per_segment: list[SegmentSentimentV1] = Field(default_factory=list)


class EnrichedDocumentV1(BaseModel):
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
    entities: list[EntityV1] = Field(default_factory=list)
    keywords: list[KeywordV1] = Field(default_factory=list)
    topics: list[TopicScoreV1] = Field(default_factory=list)
    sentiment: SentimentBundleV1 = Field(default_factory=SentimentBundleV1)
    embedding: list[float] = Field(default_factory=list)
    enriched_at: str | None = None

