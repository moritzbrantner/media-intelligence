#!/usr/bin/env python3
"""Entrypoint for transcription preprocessing."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path

from .pipeline import PipelineConfig, run_preprocessing


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Transcribe and diarize media into utterances")
    parser.add_argument("media_path", type=Path, help="Path to raw media file (audio or video)")
    parser.add_argument(
        "--artifact-dir",
        type=Path,
        default=Path("artifacts/transcription"),
        help="Directory where raw + normalized artifacts are saved",
    )
    parser.add_argument(
        "--whisper-model",
        default=os.getenv("WHISPER_MODEL", "openai/whisper-small"),
        help="HF whisper model name",
    )
    parser.add_argument(
        "--pyannote-model",
        default=os.getenv("PYANNOTE_MODEL", "pyannote/speaker-diarization-3.1"),
        help="HF pyannote diarization model name",
    )
    parser.add_argument("--hf-token", default=os.getenv("HF_TOKEN"), help="Hugging Face access token")
    parser.add_argument("--device", default=os.getenv("TRANSCRIPTION_DEVICE", "cpu"), choices=["cpu", "cuda"])
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    config = PipelineConfig(
        whisper_model=args.whisper_model,
        pyannote_model=args.pyannote_model,
        hf_token=args.hf_token,
        device=args.device,
    )
    result = run_preprocessing(args.media_path, args.artifact_dir, config)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()

