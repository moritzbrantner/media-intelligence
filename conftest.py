from __future__ import annotations

import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent
PATHS = [
    ROOT / "contracts" / "src",
    ROOT / "projects" / "discovery" / "src",
    ROOT / "projects" / "extraction" / "src",
    ROOT / "projects" / "search" / "src",
    ROOT / "projects" / "understanding" / "src",
]

for path in reversed(PATHS):
    as_text = str(path)
    if path.exists() and as_text not in sys.path:
        sys.path.insert(0, as_text)
