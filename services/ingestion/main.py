#!/usr/bin/env python3
"""Compatibility entrypoint for `mi_discovery.ingestion.main`."""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
for path in [ROOT / "contracts" / "src", ROOT / "projects" / "discovery" / "src"]:
    as_text = str(path)
    if path.exists() and as_text not in sys.path:
        sys.path.insert(0, as_text)

from mi_discovery.ingestion.main import *  # noqa: F401,F403


if __name__ == "__main__":
    main()
