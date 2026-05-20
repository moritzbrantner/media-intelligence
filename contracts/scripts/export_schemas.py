from __future__ import annotations

import json
from pathlib import Path

from mi_contracts import ContentDocumentV1, EnrichedDocumentV1, EventEnvelopeV1, MediaObjectV1


SCHEMA_MODELS = {
    "content_document_v1": ContentDocumentV1,
    "enriched_document_v1": EnrichedDocumentV1,
    "event_envelope_v1": EventEnvelopeV1,
    "media_object_v1": MediaObjectV1,
}


def main() -> None:
    output_dir = Path(__file__).resolve().parents[1] / "schemas" / "v1"
    output_dir.mkdir(parents=True, exist_ok=True)
    for filename, model in SCHEMA_MODELS.items():
        path = output_dir / f"{filename}.json"
        path.write_text(json.dumps(model.model_json_schema(), indent=2), encoding="utf-8")
        print(f"wrote {path}")


if __name__ == "__main__":
    main()

