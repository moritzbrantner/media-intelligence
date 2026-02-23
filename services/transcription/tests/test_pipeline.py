from services.transcription.pipeline import (
    NormalizedUtterance,
    PunctuationRestorer,
    apply_punctuation_and_sentence_split,
    merge_tiny_adjacent_fragments,
    normalize_utterances,
)


def test_normalize_utterances_assigns_speaker_by_overlap() -> None:
    asr = {
        "chunks": [
            {"timestamp": [0.0, 1.0], "text": "hello", "confidence": 0.8},
            {"timestamp": [1.2, 2.0], "text": "world", "confidence": 0.9},
        ]
    }
    turns = [
        {"speaker_id": "SPEAKER_00", "start_ms": 0, "end_ms": 1100},
        {"speaker_id": "SPEAKER_01", "start_ms": 1100, "end_ms": 2200},
    ]

    out = normalize_utterances(asr, turns)

    assert len(out) == 2
    assert out[0].speaker_id == "SPEAKER_00"
    assert out[1].speaker_id == "SPEAKER_01"


def test_merge_tiny_adjacent_fragments_merges_same_speaker() -> None:
    utterances = [
        NormalizedUtterance("1", "A", 0, 1000, "first", 0.9),
        NormalizedUtterance("2", "A", 1050, 1300, "tiny", 0.8),
        NormalizedUtterance("3", "B", 1400, 2000, "other", 0.7),
    ]

    out = merge_tiny_adjacent_fragments(
        utterances,
        min_fragment_ms=400,
        max_same_speaker_gap_ms=200,
    )

    assert len(out) == 2
    assert out[0].text == "first tiny"
    assert out[0].end_ms == 1300


def test_apply_punctuation_and_sentence_split() -> None:
    utterances = [
        NormalizedUtterance("1", "A", 0, 1000, "hello world how are you", 0.8),
    ]

    out = apply_punctuation_and_sentence_split(utterances, PunctuationRestorer())

    assert len(out) >= 1
    assert out[0].text
