#!/usr/bin/env python3
"""Evaluate WER + diarization error on a labeled sample."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


def _load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def _simple_wer(reference: str, hypothesis: str) -> float:
    try:
        from jiwer import wer

        return float(wer(reference, hypothesis))
    except Exception:
        ref_tokens = reference.split()
        hyp_tokens = hypothesis.split()
        if not ref_tokens:
            return 0.0 if not hyp_tokens else 1.0
        mismatches = sum(1 for a, b in zip(ref_tokens, hyp_tokens) if a != b)
        mismatches += abs(len(ref_tokens) - len(hyp_tokens))
        return mismatches / len(ref_tokens)


def _overlap_ms(a_start: int, a_end: int, b_start: int, b_end: int) -> int:
    return max(0, min(a_end, b_end) - max(a_start, b_start))


def _simple_diarization_error(
    reference: list[dict[str, Any]], hypothesis: list[dict[str, Any]]
) -> float:
    total_ref = sum(max(0, r["end_ms"] - r["start_ms"]) for r in reference)
    if total_ref <= 0:
        return 0.0

    matched = 0
    for ref in reference:
        best = 0
        for hyp in hypothesis:
            if hyp["speaker_id"] != ref["speaker_id"]:
                continue
            best = max(best, _overlap_ms(ref["start_ms"], ref["end_ms"], hyp["start_ms"], hyp["end_ms"]))
        matched += best

    return max(0.0, 1.0 - (matched / total_ref))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--reference",
        type=Path,
        default=Path("services/transcription/tests/fixtures/labeled_sample.json"),
    )
    parser.add_argument(
        "--hypothesis",
        type=Path,
        default=Path("artifacts/transcription/normalized_utterances.json"),
    )
    args = parser.parse_args()

    reference = _load_json(args.reference)
    hypothesis = _load_json(args.hypothesis)

    reference_text = " ".join(item["text"] for item in reference)
    hypothesis_text = " ".join(item["text"] for item in hypothesis)
    wer_value = _simple_wer(reference_text, hypothesis_text)

    der_value = _simple_diarization_error(reference, hypothesis)

    print(json.dumps({"wer": round(wer_value, 4), "der": round(der_value, 4)}, indent=2))


if __name__ == "__main__":
    main()
