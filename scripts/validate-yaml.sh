#!/usr/bin/env bash
set -euo pipefail

FILES=(
  "infrastructure/inference/self_hosted.endpoints.yaml"
  "infrastructure/inference/hf_dedicated.endpoints.yaml"
)

have_python_yaml=0
if command -v python >/dev/null 2>&1; then
  if python - <<'PY' >/dev/null 2>&1
import yaml  # noqa: F401
PY
  then
    have_python_yaml=1
  fi
fi

if [ "$have_python_yaml" -eq 1 ]; then
  python - <<'PY'
import yaml
files = [
  "infrastructure/inference/self_hosted.endpoints.yaml",
  "infrastructure/inference/hf_dedicated.endpoints.yaml",
]
for f in files:
  with open(f, "r", encoding="utf-8") as handle:
    yaml.safe_load(handle)
print("[yaml-validation] ok (python/PyYAML)")
PY
  exit 0
fi

if command -v ruby >/dev/null 2>&1; then
  ruby -e 'require "yaml"; YAML.load_file("infrastructure/inference/self_hosted.endpoints.yaml"); YAML.load_file("infrastructure/inference/hf_dedicated.endpoints.yaml"); puts "[yaml-validation] ok (ruby)"'
  echo "[yaml-validation] warning: PyYAML missing; install with: pip install -r requirements-dev.txt" >&2
  exit 0
fi

echo "[yaml-validation] error: neither Python+PyYAML nor Ruby is available for YAML parsing" >&2
exit 1
