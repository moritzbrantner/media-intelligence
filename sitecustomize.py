from __future__ import annotations

import os
import sys
from pathlib import Path


os.environ.setdefault("PYTEST_DISABLE_PLUGIN_AUTOLOAD", "1")

ROOT = Path(__file__).resolve().parent
EXTRA_PATHS = [
    ROOT / "contracts" / "src",
    ROOT / "projects" / "discovery" / "src",
    ROOT / "projects" / "extraction" / "src",
    ROOT / "projects" / "search" / "src",
    ROOT / "projects" / "understanding" / "src",
]

for path in reversed(EXTRA_PATHS):
    as_text = str(path)
    if path.exists() and as_text not in sys.path:
        sys.path.insert(0, as_text)

