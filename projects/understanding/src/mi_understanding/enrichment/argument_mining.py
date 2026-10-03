from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
import logging
import re
from typing import Protocol
import uuid

logger = logging.getLogger(__name__)

ASSERTIVE_VERBS = {
    "is",
    "are",
    "will",
    "must",
    "should",
    "need",
    "proves",
    "means",
    "shows",
}
EVIDENCE_MARKERS = {
    "because",
    "since",
    "for example",
    "for instance",
    "evidence",
    "data shows",
    "according to",
    "study",
    "research",
}


class StanceLabel(str, Enum):
    PRO = "pro"
    CON = "con"
    NEUTRAL = "neutral"


@dataclass(slots=True)
class TranscriptSegment:
    """Single speaker-labeled transcript segment."""

    segment_index: int
    speaker_id: str
    text: str


@dataclass(slots=True)
class ArgumentUnit:
    """Structured argument object emitted by argument mining stage."""

    argument_id: str
    speaker_id: str
    topic_id: str
    claim_text: str
    evidence_spans: list[str]
    stance: StanceLabel
    confidence: float


@dataclass(slots=True)
class ReviewedArgumentSample:
    """Human-reviewed sample used for threshold calibration."""

    entailment_score: float
    contradiction_score: float
    topic_related_score: float
    is_true_argument: bool


@dataclass(slots=True)
class ThresholdConfig:
    """Threshold set used by the argument extraction stage."""

    min_claim_topic_entailment: float = 0.45
    max_claim_topic_contradiction: float = 0.45
    min_claim_evidence_entailment: float = 0.45
    min_confidence: float = 0.45


@dataclass(slots=True)
class PairwiseScore:
    entailment: float
    contradiction: float
    neutral: float


class PairwiseScorer(Protocol):
    """Protocol for claim/evidence and claim/topic NLI scoring."""

    def score_pair(self, premise: str, hypothesis: str) -> PairwiseScore:
        ...


class StanceScorer(Protocol):
    """Protocol for zero-shot stance scoring against a topic."""

    def score_stance(self, text: str, topic_statement: str) -> dict[StanceLabel, float]:
        ...


class HFNliScorer:
    """HuggingFace NLI scorer built on MNLI-family checkpoints."""

    def __init__(self, model_name: str = "MoritzLaurer/DeBERTa-v3-large-mnli-fever-anli-ling-wanli"):
        from transformers import pipeline

        self._pipeline = pipeline(
            "text-classification",
            model=model_name,
            tokenizer=model_name,
            top_k=None,
            truncation=True,
        )

    def score_pair(self, premise: str, hypothesis: str) -> PairwiseScore:
        output = self._pipeline({"text": premise, "text_pair": hypothesis})
        label_map = {item["label"].lower(): float(item["score"]) for item in output}
        return PairwiseScore(
            entailment=label_map.get("entailment", 0.0),
            contradiction=label_map.get("contradiction", 0.0),
            neutral=label_map.get("neutral", 0.0),
        )


class HFZeroShotStanceScorer:
    """HuggingFace zero-shot classifier used for pro/con/neutral stance labels."""

    def __init__(self, model_name: str = "roberta-large-mnli"):
        from transformers import pipeline

        self._pipeline = pipeline("zero-shot-classification", model=model_name, tokenizer=model_name)

    def score_stance(self, text: str, topic_statement: str) -> dict[StanceLabel, float]:
        result = self._pipeline(
            text,
            candidate_labels=["pro", "con", "neutral"],
            hypothesis_template=f"This statement is {{}} the topic: {topic_statement}",
            multi_label=False,
        )
        raw = {label.lower(): float(score) for label, score in zip(result["labels"], result["scores"])}
        return {
            StanceLabel.PRO: raw.get("pro", 0.0),
            StanceLabel.CON: raw.get("con", 0.0),
            StanceLabel.NEUTRAL: raw.get("neutral", 0.0),
        }


def split_sentences(text: str) -> list[str]:
    """Lightweight sentence splitter for candidate generation."""

    chunks = [s.strip() for s in re.split(r"(?<=[.!?])\s+", text) if s.strip()]
    return chunks if chunks else ([text.strip()] if text.strip() else [])


def is_claim_candidate(sentence: str) -> bool:
    lowered = sentence.lower()
    if "?" in sentence:
        return False
    has_assertive_verb = any(re.search(rf"\b{re.escape(verb)}\b", lowered) for verb in ASSERTIVE_VERBS)
    has_length = len(sentence.split()) >= 6
    return has_assertive_verb and has_length


def is_evidence_candidate(sentence: str) -> bool:
    lowered = sentence.lower()
    return any(marker in lowered for marker in EVIDENCE_MARKERS)


def confidence_from_scores(
    claim_topic_score: PairwiseScore,
    claim_evidence_score: PairwiseScore,
    stance_distribution: dict[StanceLabel, float],
) -> float:
    stance_confidence = max(stance_distribution.values()) if stance_distribution else 0.0
    raw = (
        claim_topic_score.entailment
        + (1.0 - claim_topic_score.contradiction)
        + claim_evidence_score.entailment
        + stance_confidence
    ) / 4.0
    return max(0.0, min(1.0, raw))


class ArgumentMiningStage:
    """Pipeline stage that turns speaker-labeled transcript segments into argument units."""

    def __init__(
        self,
        nli_scorer: PairwiseScorer,
        stance_scorer: StanceScorer,
        thresholds: ThresholdConfig | None = None,
    ):
        self.nli_scorer = nli_scorer
        self.stance_scorer = stance_scorer
        self.thresholds = thresholds or ThresholdConfig()

    def run(
        self,
        segments: list[TranscriptSegment],
        topic_id: str,
        topic_statement: str,
    ) -> list[ArgumentUnit]:
        """Extract argument units after transcript segmentation."""

        argument_units: list[ArgumentUnit] = []

        for segment in segments:
            sentences = split_sentences(segment.text)
            claim_candidates = [sentence for sentence in sentences if is_claim_candidate(sentence)]
            evidence_candidates = [sentence for sentence in sentences if is_evidence_candidate(sentence)]

            for claim_text in claim_candidates:
                claim_topic_score = self.nli_scorer.score_pair(claim_text, topic_statement)
                if claim_topic_score.entailment < self.thresholds.min_claim_topic_entailment:
                    continue
                if claim_topic_score.contradiction > self.thresholds.max_claim_topic_contradiction:
                    continue

                evidence_spans: list[str] = []
                best_evidence_entailment = 0.0
                for evidence_sentence in evidence_candidates:
                    pair_score = self.nli_scorer.score_pair(claim_text, evidence_sentence)
                    if pair_score.entailment >= self.thresholds.min_claim_evidence_entailment:
                        evidence_spans.append(evidence_sentence)
                    best_evidence_entailment = max(best_evidence_entailment, pair_score.entailment)

                if not evidence_spans:
                    continue

                stance_distribution = self.stance_scorer.score_stance(claim_text, topic_statement)
                stance = max(stance_distribution, key=stance_distribution.get)

                confidence = confidence_from_scores(
                    claim_topic_score=claim_topic_score,
                    claim_evidence_score=PairwiseScore(
                        entailment=best_evidence_entailment,
                        contradiction=0.0,
                        neutral=0.0,
                    ),
                    stance_distribution=stance_distribution,
                )
                if confidence < self.thresholds.min_confidence:
                    continue

                argument_units.append(
                    ArgumentUnit(
                        argument_id=str(uuid.uuid4()),
                        speaker_id=segment.speaker_id,
                        topic_id=topic_id,
                        claim_text=claim_text,
                        evidence_spans=evidence_spans,
                        stance=stance,
                        confidence=confidence,
                    )
                )

        return argument_units


def calibrate_thresholds(reviewed_samples: list[ReviewedArgumentSample]) -> ThresholdConfig:
    """Grid-search thresholds over manually reviewed samples to reduce false positives."""

    if not reviewed_samples:
        logger.warning("No reviewed samples supplied; using default thresholds.")
        return ThresholdConfig()

    best_config = ThresholdConfig()
    best_f1 = -1.0

    for topic_entailment in (0.35, 0.4, 0.45, 0.5, 0.55, 0.6):
        for topic_contradiction in (0.3, 0.35, 0.4, 0.45, 0.5):
            for evidence_entailment in (0.35, 0.4, 0.45, 0.5, 0.55):
                tp = fp = fn = 0
                for sample in reviewed_samples:
                    predicted = (
                        sample.topic_related_score >= topic_entailment
                        and sample.contradiction_score <= topic_contradiction
                        and sample.entailment_score >= evidence_entailment
                    )
                    if predicted and sample.is_true_argument:
                        tp += 1
                    elif predicted and not sample.is_true_argument:
                        fp += 1
                    elif not predicted and sample.is_true_argument:
                        fn += 1

                precision = tp / (tp + fp) if (tp + fp) else 0.0
                recall = tp / (tp + fn) if (tp + fn) else 0.0
                f1 = (2 * precision * recall / (precision + recall)) if (precision + recall) else 0.0

                if f1 > best_f1:
                    best_f1 = f1
                    best_config = ThresholdConfig(
                        min_claim_topic_entailment=topic_entailment,
                        max_claim_topic_contradiction=topic_contradiction,
                        min_claim_evidence_entailment=evidence_entailment,
                        min_confidence=0.45,
                    )

    logger.info("Selected calibrated thresholds with F1=%.3f: %s", best_f1, best_config)
    return best_config

