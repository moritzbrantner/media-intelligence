from __future__ import annotations

import os
import runpy
import sys


os.environ.setdefault("PYTEST_DISABLE_PLUGIN_AUTOLOAD", "1")

CURRENT = sys.path.pop(0)
try:
    runpy.run_module("pytest", run_name="__main__", alter_sys=True)
finally:
    sys.path.insert(0, CURRENT)

