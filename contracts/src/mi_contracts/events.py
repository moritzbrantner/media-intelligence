from __future__ import annotations

from pydantic import BaseModel, Field


class EventMetadataV1(BaseModel):
    schema_version: str = "v1"
    source_type: str
    retry_count: int = 0


class EventEnvelopeV1(BaseModel):
    schema_version: str = "v1"
    event_id: str
    event_type: str
    timestamp: str
    source_id: str
    payload_location: str | None = None
    metadata: EventMetadataV1 = Field(default_factory=EventMetadataV1)

