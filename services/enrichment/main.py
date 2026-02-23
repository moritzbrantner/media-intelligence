#!/usr/bin/env python3
"""Mock entrypoint for the enrichment service."""

from __future__ import annotations

import os
import pathlib
import runpy


if __name__ == "__main__":
    os.environ.setdefault("SERVICE_NAME", "mock-enrichment")
    os.environ.setdefault("MOCK_SOURCE", "enrichment")
    os.environ.setdefault("MOCK_SERVICE_PORT", "8084")
    script_path = pathlib.Path(__file__).resolve().parents[2] / "scripts" / "mock_service.py"
    runpy.run_path(str(script_path), run_name="__main__")
