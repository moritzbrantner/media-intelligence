from __future__ import annotations

from datetime import datetime, timezone
import hashlib


from services.enrichment.argument_mining import StanceLabel
from services.enrichment.retrieval import (
    ArgumentRecord,
    ArgumentRetrievalSubsystem,
    BenchmarkReport,
    NumpyANNIndex,
    OfflineRelevanceBenchmark,
    RetrievalFilters,
    RetrievalQuery,
    SimilarArgumentQuery,
)


class FakeBiEncoder:
    def __init__(self, dimension: int = 24):
        self._dimension = dimension

    def encode(self, texts: list[str]) -> list[list[float]]:
        vectors = []
        for text in texts:
            digest = hashlib.sha256(text.encode("utf-8")).digest()
            vec = [float(byte) for byte in digest[: self._dimension]]
            vectors.append(vec)
        return vectors


class KeywordCrossEncoder:
    def score(self, query_text: str, candidate_texts: list[str]) -> list[float]:
        query_tokens = {token.lower() for token in query_text.split()}
        scores = []
        for candidate in candidate_texts:
            candidate_tokens = {token.lower() for token in candidate.split()}
            overlap = len(query_tokens.intersection(candidate_tokens))
            scores.append(float(overlap))
        return scores


def _build_subsystem() -> ArgumentRetrievalSubsystem:
    subsystem = ArgumentRetrievalSubsystem(
        bi_encoder=FakeBiEncoder(),
        cross_encoder=KeywordCrossEncoder(),
        ann_index=NumpyANNIndex(),
    )
    subsystem.upsert_arguments(
        [
            ArgumentRecord(
                argument_id="arg-1",
                speaker_id="speaker-a",
                topic_id="climate",
                claim_text="Carbon pricing is effective at reducing emissions",
                context_text="Long-run data from OECD markets shows clear declines",
                stance=StanceLabel.PRO,
                source_type="podcast",
                published_at=datetime(2024, 1, 10, tzinfo=timezone.utc),
            ),
            ArgumentRecord(
                argument_id="arg-2",
                speaker_id="speaker-b",
                topic_id="climate",
                claim_text="Carbon taxes raise costs without reducing net emissions",
                context_text="Manufacturers shift production and prices rise for households",
                stance=StanceLabel.CON,
                source_type="debate",
                published_at=datetime(2024, 1, 15, tzinfo=timezone.utc),
            ),
            ArgumentRecord(
                argument_id="arg-3",
                speaker_id="speaker-c",
                topic_id="energy",
                claim_text="Grid storage subsidies improve renewable reliability",
                context_text="Battery deployment smooths supply variability",
                stance=StanceLabel.PRO,
                source_type="article",
                published_at=datetime(2022, 3, 5, tzinfo=timezone.utc),
            ),
        ]
    )
    return subsystem


def test_retrieval_applies_hybrid_scoring_and_returns_ranked_results():
    subsystem = _build_subsystem()

    query = RetrievalQuery(
        query_argument_id="query-1",
        text="Carbon pricing data shows emissions decline in regulated markets",
        topic_id="climate",
        stance=StanceLabel.PRO,
        published_at=datetime(2024, 1, 20, tzinfo=timezone.utc),
    )

    results = subsystem.retrieve(query=query, top_k=2)

    assert len(results) == 2
    assert results[0].argument.argument_id == "arg-1"
    assert results[0].score.final_score >= results[1].score.final_score
    assert results[0].score.semantic_score >= -1.0


def test_retrieval_supports_topic_stance_speaker_date_source_filters():
    subsystem = _build_subsystem()
    query = RetrievalQuery(query_argument_id="query-1", text="carbon tax emissions")

    filters = RetrievalFilters(
        topic_ids={"climate"},
        stances={StanceLabel.CON},
        speaker_ids={"speaker-b"},
        source_types={"debate"},
        start_date=datetime(2024, 1, 1, tzinfo=timezone.utc),
        end_date=datetime(2024, 12, 31, tzinfo=timezone.utc),
    )
    results = subsystem.retrieve(query=query, top_k=5, filters=filters)

    assert [item.argument.argument_id for item in results] == ["arg-2"]


def test_offline_benchmark_reports_hit_rate_and_mrr():
    subsystem = _build_subsystem()
    benchmark = OfflineRelevanceBenchmark(subsystem)

    query_lookup = {
        "q1": RetrievalQuery(query_argument_id="q1", text="carbon pricing lowers emissions", topic_id="climate"),
        "q2": RetrievalQuery(query_argument_id="q2", text="battery storage supports grid reliability", topic_id="energy"),
    }
    query_set = [
        SimilarArgumentQuery(query_argument_id="q1", relevant_argument_ids={"arg-1"}),
        SimilarArgumentQuery(query_argument_id="q2", relevant_argument_ids={"arg-3"}),
    ]

    report = benchmark.run(query_set=query_set, query_lookup=query_lookup, top_k=2)

    assert isinstance(report, BenchmarkReport)
    assert report.evaluated_queries == 2
    assert report.hit_rate_at_k > 0
    assert report.mean_reciprocal_rank > 0
