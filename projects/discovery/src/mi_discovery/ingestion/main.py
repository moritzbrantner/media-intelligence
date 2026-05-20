#!/usr/bin/env python3
"""Mock entrypoint for the ingestion service."""

from __future__ import annotations

import os
import pathlib
import runpy


def main() -> None:
    os.environ.setdefault("SERVICE_NAME", "mock-ingestion")
    os.environ.setdefault("MOCK_SOURCE", "ingestion")
    os.environ.setdefault("MOCK_SERVICE_PORT", "8081")
    script_path = pathlib.Path(__file__).resolve().parents[4] / "ops" / "scripts" / "mock_service.py"
    runpy.run_path(str(script_path), run_name="__main__")


if __name__ == "__main__":
    main()

