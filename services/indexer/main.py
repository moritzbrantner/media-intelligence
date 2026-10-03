#!/usr/bin/env python3
"""Compatibility entrypoint for the mock indexer service."""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
as_text = str(ROOT)
if as_text not in sys.path:
    sys.path.insert(0, as_text)

from ops.scripts.mock_service import *  # noqa: F401,F403


if __name__ == "__main__":
    main()
