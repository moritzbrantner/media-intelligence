from __future__ import annotations

from mi_understanding.enrichment.argument_mining import (
    ArgumentMiningStage,
    PairwiseScore,
    ReviewedArgumentSample,
    StanceLabel,
    ThresholdConfig,
    TranscriptSegment,
    calibrate_thresholds,
)


class FakeNliScorer:
    def score_pair(self, premise: str, hypothesis: str) -> PairwiseScore:
        p = premise.lower()
        h = hypothesis.lower()
        if "carbon tax" in p and "because" in h:
            return PairwiseScore(entailment=0.81, contradiction=0.08, neutral=0.11)
        if "carbon tax" in p and "climate policy" in h:
            return PairwiseScore(entailment=0.77, contradiction=0.07, neutral=0.16)
        return PairwiseScore(entailment=0.2, contradiction=0.5, neutral=0.3)


class FakeStanceScorer:
    def score_stance(self, text: str, topic_statement: str) -> dict[StanceLabel, float]:
        if "carbon tax" in text.lower():
            return {StanceLabel.PRO: 0.76, StanceLabel.CON: 0.12, StanceLabel.NEUTRAL: 0.12}
        return {StanceLabel.PRO: 0.2, StanceLabel.CON: 0.2, StanceLabel.NEUTRAL: 0.6}


def test_extracts_argument_units_from_segment():
    stage = ArgumentMiningStage(
        nli_scorer=FakeNliScorer(),
        stance_scorer=FakeStanceScorer(),
        thresholds=ThresholdConfig(
            min_claim_topic_entailment=0.6,
            max_claim_topic_contradiction=0.2,
            min_claim_evidence_entailment=0.7,
            min_confidence=0.5,
        ),
    )

    segments = [
        TranscriptSegment(
            segment_index=0,
            speaker_id="SPEAKER_01",
            text=(
                "A carbon tax is an effective climate policy because data shows emissions drop "
                "after it is introduced."
            ),
        )
    ]

    results = stage.run(segments=segments, topic_id="topic-climate", topic_statement="climate policy")

    assert len(results) == 1
    result = results[0]
    assert result.speaker_id == "SPEAKER_01"
    assert result.topic_id == "topic-climate"
    assert result.claim_text.startswith("A carbon tax is")
    assert result.evidence_spans
    assert result.stance == StanceLabel.PRO
    assert result.confidence >= 0.5


def test_calibrate_thresholds_prefers_settings_with_fewer_false_positives():
    samples = [
        ReviewedArgumentSample(0.8, 0.1, 0.8, True),
        ReviewedArgumentSample(0.78, 0.12, 0.76, True),
        ReviewedArgumentSample(0.35, 0.55, 0.3, False),
        ReviewedArgumentSample(0.4, 0.5, 0.42, False),
    ]

    thresholds = calibrate_thresholds(samples)

    assert thresholds.min_claim_topic_entailment >= 0.35
    assert thresholds.max_claim_topic_contradiction <= 0.5
    assert thresholds.min_claim_evidence_entailment >= 0.35

