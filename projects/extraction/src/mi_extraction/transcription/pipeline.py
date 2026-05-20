"""Transcription preprocessing pipeline.

Converts raw media into timestamped, speaker-labeled utterances and persists
raw + normalized artifacts so downstream jobs can be replayed without
re-transcribing.
"""

from __future__ import annotations

import json
import logging
import re
import subprocess
import uuid
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Iterable

logger = logging.getLogger(__name__)


@dataclass
class NormalizedUtterance:
    segment_id: str
    speaker_id: str
    start_ms: int
    end_ms: int
    text: str
    confidence: float


@dataclass
class PipelineConfig:
    whisper_model: str = "openai/whisper-small"
    pyannote_model: str = "pyannote/speaker-diarization-3.1"
    hf_token: str | None = None
    device: str = "cpu"
    min_fragment_ms: int = 450
    max_same_speaker_gap_ms: int = 300


class ASRModel:
    """Whisper-family ASR model backed by Hugging Face transformers."""

    def __init__(self, model_name: str, device: str = "cpu") -> None:
        from transformers import pipeline

        device_idx = -1 if device == "cpu" else 0
        self._pipeline = pipeline(
            task="automatic-speech-recognition",
            model=model_name,
            device=device_idx,
            return_timestamps=True,
        )

    def transcribe(self, wav_path: Path) -> dict[str, Any]:
        result: dict[str, Any] = self._pipeline(str(wav_path))
        return result


class DiarizationModel:
    """pyannote speaker diarization model."""

    def __init__(self, model_name: str, hf_token: str | None = None) -> None:
        from pyannote.audio import Pipeline

        self._pipeline = Pipeline.from_pretrained(model_name, use_auth_token=hf_token)

    def diarize(self, wav_path: Path) -> Any:
        return self._pipeline(str(wav_path))


class PunctuationRestorer:
    """Optional punctuation restoration with a lightweight HF model."""

    def __init__(self) -> None:
        self._model = None
        try:
            from deepmultilingualpunctuation import PunctuationModel

            self._model = PunctuationModel()
        except Exception as exc:  # pragma: no cover - optional dependency
            logger.warning("Punctuation model unavailable: %s", exc)

    def restore(self, text: str) -> str:
        cleaned = " ".join(text.split()).strip()
        if not cleaned:
            return cleaned
        if self._model is not None:
            try:
                return self._model.restore_punctuation(cleaned)
            except Exception as exc:  # pragma: no cover - optional dependency
                logger.warning("Punctuation restore failed, using fallback: %s", exc)
        if cleaned[-1] not in ".!?":
            cleaned = f"{cleaned}."
        return cleaned[0].upper() + cleaned[1:]

    @staticmethod
    def split_sentences(text: str) -> list[str]:
        parts = [p.strip() for p in re.split(r"(?<=[.!?])\s+", text) if p.strip()]
        return parts or ([text.strip()] if text.strip() else [])


def _to_ms(seconds: float | None) -> int:
    return int(round((seconds or 0.0) * 1000))


def _overlap_ms(start_a: int, end_a: int, start_b: int, end_b: int) -> int:
    return max(0, min(end_a, end_b) - max(start_a, start_b))


def normalize_utterances(
    asr_output: dict[str, Any],
    diarization_turns: list[dict[str, Any]],
) -> list[NormalizedUtterance]:
    normalized: list[NormalizedUtterance] = []

    chunks = asr_output.get("chunks", [])
    for chunk in chunks:
        start_ms = _to_ms((chunk.get("timestamp") or [0, 0])[0])
        end_ms = _to_ms((chunk.get("timestamp") or [0, 0])[1])
        text = (chunk.get("text") or "").strip()
        if not text:
            continue

        best_speaker = "SPEAKER_UNKNOWN"
        best_overlap = 0
        for turn in diarization_turns:
            overlap = _overlap_ms(start_ms, end_ms, turn["start_ms"], turn["end_ms"])
            if overlap > best_overlap:
                best_overlap = overlap
                best_speaker = turn["speaker_id"]

        confidence = float(chunk.get("confidence", 1.0))
        normalized.append(
            NormalizedUtterance(
                segment_id=str(uuid.uuid4()),
                speaker_id=best_speaker,
                start_ms=start_ms,
                end_ms=end_ms,
                text=text,
                confidence=confidence,
            )
        )

    return normalized


def merge_tiny_adjacent_fragments(
    utterances: Iterable[NormalizedUtterance],
    min_fragment_ms: int,
    max_same_speaker_gap_ms: int,
) -> list[NormalizedUtterance]:
    items = sorted(utterances, key=lambda item: item.start_ms)
    if not items:
        return []

    merged: list[NormalizedUtterance] = [items[0]]
    for current in items[1:]:
        prev = merged[-1]
        current_duration = current.end_ms - current.start_ms
        gap_ms = current.start_ms - prev.end_ms
        if (
            prev.speaker_id == current.speaker_id
            and current_duration <= min_fragment_ms
            and gap_ms <= max_same_speaker_gap_ms
        ):
            prev.end_ms = max(prev.end_ms, current.end_ms)
            prev.text = f"{prev.text} {current.text}".strip()
            prev.confidence = (prev.confidence + current.confidence) / 2
            continue
        merged.append(current)
    return merged


def apply_punctuation_and_sentence_split(
    utterances: Iterable[NormalizedUtterance],
    punctuator: PunctuationRestorer,
) -> list[NormalizedUtterance]:
    output: list[NormalizedUtterance] = []
    for utterance in utterances:
        punctuated = punctuator.restore(utterance.text)
        sentences = punctuator.split_sentences(punctuated)
        if len(sentences) <= 1:
            utterance.text = punctuated
            output.append(utterance)
            continue

        duration = max(1, utterance.end_ms - utterance.start_ms)
        cursor = utterance.start_ms
        per_sentence = max(1, duration // len(sentences))
        for index, sentence in enumerate(sentences):
            start_ms = cursor
            end_ms = utterance.end_ms if index == len(sentences) - 1 else min(utterance.end_ms, start_ms + per_sentence)
            output.append(
                NormalizedUtterance(
                    segment_id=str(uuid.uuid4()),
                    speaker_id=utterance.speaker_id,
                    start_ms=start_ms,
                    end_ms=end_ms,
                    text=sentence,
                    confidence=utterance.confidence,
                )
            )
            cursor = end_ms
    return output


def ensure_wav_16khz_mono(input_media_path: Path, output_wav_path: Path) -> Path:
    output_wav_path.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        "ffmpeg",
        "-y",
        "-i",
        str(input_media_path),
        "-ac",
        "1",
        "-ar",
        "16000",
        str(output_wav_path),
    ]
    subprocess.run(cmd, check=True, capture_output=True)
    return output_wav_path


def diarization_to_turns(diarization: Any) -> list[dict[str, Any]]:
    turns: list[dict[str, Any]] = []
    for turn, _, speaker in diarization.itertracks(yield_label=True):
        turns.append(
            {
                "speaker_id": str(speaker),
                "start_ms": _to_ms(turn.start),
                "end_ms": _to_ms(turn.end),
            }
        )
    return turns


def persist_artifacts(
    output_dir: Path,
    asr_output: dict[str, Any],
    diarization_turns: list[dict[str, Any]],
    utterances: list[NormalizedUtterance],
) -> dict[str, str]:
    output_dir.mkdir(parents=True, exist_ok=True)

    asr_path = output_dir / "raw_asr.json"
    diar_path = output_dir / "raw_diarization.json"
    normalized_path = output_dir / "normalized_utterances.json"

    asr_path.write_text(json.dumps(asr_output, indent=2), encoding="utf-8")
    diar_path.write_text(json.dumps(diarization_turns, indent=2), encoding="utf-8")
    normalized_path.write_text(
        json.dumps([asdict(item) for item in utterances], indent=2), encoding="utf-8"
    )

    return {
        "asr": str(asr_path),
        "diarization": str(diar_path),
        "normalized": str(normalized_path),
    }


def run_preprocessing(
    media_path: Path,
    artifact_dir: Path,
    config: PipelineConfig,
) -> dict[str, Any]:
    wav_path = ensure_wav_16khz_mono(media_path, artifact_dir / "normalized_audio.wav")

    asr_model = ASRModel(model_name=config.whisper_model, device=config.device)
    diar_model = DiarizationModel(model_name=config.pyannote_model, hf_token=config.hf_token)

    asr_output = asr_model.transcribe(wav_path)
    diarization = diar_model.diarize(wav_path)
    diarization_turns = diarization_to_turns(diarization)

    normalized = normalize_utterances(asr_output, diarization_turns)
    normalized = merge_tiny_adjacent_fragments(
        normalized,
        min_fragment_ms=config.min_fragment_ms,
        max_same_speaker_gap_ms=config.max_same_speaker_gap_ms,
    )
    normalized = apply_punctuation_and_sentence_split(normalized, PunctuationRestorer())

    artifacts = persist_artifacts(artifact_dir, asr_output, diarization_turns, normalized)
    return {
        "utterance_count": len(normalized),
        "artifacts": artifacts,
    }

