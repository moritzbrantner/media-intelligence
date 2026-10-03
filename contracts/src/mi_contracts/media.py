from __future__ import annotations

from pydantic import BaseModel, Field

from .content import AuthorV1, ChannelV1


class MediaFileV1(BaseModel):
    type: str
    storage_path: str
    mime_type: str
    size_bytes: int


class MediaObjectV1(BaseModel):
    schema_version: str = "v1"
    id: str
    source_type: str
    source_url: str
    source_id: str
    title: str | None = None
    description: str | None = None
    author: AuthorV1 = Field(default_factory=AuthorV1)
    channel: ChannelV1 = Field(default_factory=ChannelV1)
    published_at: str | None = None
    fetched_at: str
    duration_seconds: float | None = None
    language: str | None = None
    tags: list[str] = Field(default_factory=list)
    media_files: list[MediaFileV1] = Field(default_factory=list)
    raw_metadata: dict = Field(default_factory=dict)

