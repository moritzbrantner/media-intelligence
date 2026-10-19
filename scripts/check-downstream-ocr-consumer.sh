#!/usr/bin/env bash
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
revision="$(git rev-parse HEAD)"
temp_root="$(cd "${TMPDIR:-/tmp}" && pwd -P)"
consumer_dir="$(mktemp -d "$temp_root/media-intelligence-consumer.XXXXXX")"
cleanup() {
  if [[ -d "$consumer_dir" && "$consumer_dir" == "$temp_root"/media-intelligence-consumer.* ]]; then
    rm -rf -- "$consumer_dir"
  fi
}
trap cleanup EXIT

mkdir -p "$consumer_dir/src"
python3 - "$root" "$revision" "$consumer_dir" <<'PY'
import json
import pathlib
import sys
import tomllib

root = pathlib.Path(sys.argv[1])
revision = sys.argv[2]
consumer = pathlib.Path(sys.argv[3])
workspace = tomllib.loads((root / "Cargo.toml").read_text())
semantic = workspace["patch"]["crates-io"]["moenarch-semantic-core"]
# A separate Git consumer does not inherit this workspace's patch table. Only
# this unpublished OCR primitive is needed; do not copy the entire patch graph.
manifest = f'''[package]
name = "media-intelligence-downstream-ocr-check"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
media-intelligence-analysis = {{ git = {json.dumps(root.as_uri())}, rev = {json.dumps(revision)}, features = ["visual-analysis-ocr"] }}

[patch.crates-io]
moenarch-semantic-core = {{ git = {json.dumps(semantic["git"])}, rev = {json.dumps(semantic["rev"])} }}
'''
(consumer / "Cargo.toml").write_text(manifest)
(consumer / "src/lib.rs").write_text(
    "pub use media_intelligence_analysis::{analyze, "
    "visible_text::{OcrBackendConfig, VisualAnalysisOcrProducer}};\n"
)
print(f"Checking a clean Git consumer at {revision} with only the semantic-core source override")
PY

# Dependency acquisition is explicit. The subsequent compilation is offline and locked.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target}"
cargo fetch --manifest-path "$consumer_dir/Cargo.toml"
cargo check --manifest-path "$consumer_dir/Cargo.toml" --locked --offline
