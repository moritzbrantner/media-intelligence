#!/usr/bin/env python3
"""Compatibility shim for the dev mock service."""

from __future__ import annotations

from ops.scripts.mock_service import *  # noqa: F401,F403


if __name__ == "__main__":
    main()
