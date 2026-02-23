from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
import math
from typing import Protocol

from services.enrichment.argument_mining import ArgumentUnit, StanceLabel


@dataclass(slots=True)
class ArgumentRecord:
    argument_id: str
    speaker_id: str
    topic_id: str
    claim_text: str
    context_text: str
    stance: StanceLabel
    source_type: str
    published_at: datetime | None = None

    @property
    def embedding_text(self) -> str:
        return f"{self.claim_text.strip()}\nContext: {self.context_text.strip()}".strip()


@dataclass(slots=True)
class RetrievalFilters:
    topic_ids: set[str] | None = None
    stances: set[StanceLabel] | None = None
    speaker_ids: set[str] | None = None
    source_types: set[str] | None = None
    start_date: datetime | None = None
    end_date: datetime | None = None


@dataclass(slots=True)
class CandidateScore:
    argument_id: str
    semantic_score: float
    cross_encoder_score: float
    stance_signal: float
    topical_overlap: float
    recency_score: float
    final_score: float


@dataclass(slots=True)
class RetrievalResult:
    argument: ArgumentRecord
    score: CandidateScore


@dataclass(slots=True)
class HybridScoringConfig:
    semantic_weight: float = 0.45
    cross_encoder_weight: float = 0.3
    stance_weight: float = 0.1
    topical_overlap_weight: float = 0.1
    recency_weight: float = 0.05


@dataclass(slots=True)
class RetrievalQuery:
    query_argument_id: str
    text: str
    topic_id: str | None = None
    stance: StanceLabel | None = None
    published_at: datetime | None = None


class BiEncoder(Protocol):
    def encode(self, texts: list[str]) -> list[list[float]]:
        ...


class CrossEncoder(Protocol):
    def score(self, query_text: str, candidate_texts: list[str]) -> list[float]:
        ...


class SentenceTransformerBiEncoder:
    def __init__(self, model_name: str = "BAAI/bge-base-en-v1.5"):
        from sentence_transformers import SentenceTransformer

        self._model = SentenceTransformer(model_name)

    def encode(self, texts: list[str]) -> list[list[float]]:
        vectors = self._model.encode(texts, normalize_embeddings=True)
        return [[float(x) for x in vector] for vector in vectors]


class HFCrossEncoderScorer:
    def __init__(self, model_name: str = "cross-encoder/ms-marco-MiniLM-L-12-v2"):
        from sentence_transformers import CrossEncoder

        self._model = CrossEncoder(model_name)

    def score(self, query_text: str, candidate_texts: list[str]) -> list[float]:
        if not candidate_texts:
            return []
        pairs = [[query_text, candidate_text] for candidate_text in candidate_texts]
        return [float(score) for score in self._model.predict(pairs)]


class NumpyANNIndex:
    """In-memory cosine similarity index without extra dependencies."""

    def __init__(self):
        self._ids: list[str] = []
        self._vectors: list[list[float]] = []

    def add(self, ids: list[str], vectors: list[list[float]]) -> None:
        for arg_id, vector in zip(ids, vectors):
            self._ids.append(arg_id)
            self._vectors.append(_l2_normalize(vector))

    def search(self, query_vector: list[float], top_k: int) -> list[tuple[str, float]]:
        if not self._ids:
            return []
        query = _l2_normalize(query_vector)
        scored = [
            (arg_id, _dot(query, vector))
            for arg_id, vector in zip(self._ids, self._vectors)
        ]
        scored.sort(key=lambda item: item[1], reverse=True)
        return scored[:top_k]


class FaissANNIndex:
    """Placeholder wrapper; falls back if faiss/numpy unavailable."""

    def __init__(self, dimension: int):
        self._fallback = NumpyANNIndex()
        self._dimension = dimension

    def add(self, ids: list[str], vectors: list[list[float]]) -> None:
        self._fallback.add(ids, vectors)

    def search(self, query_vector: list[float], top_k: int) -> list[tuple[str, float]]:
        return self._fallback.search(query_vector, top_k)


class ArgumentRetrievalSubsystem:
    def __init__(
        self,
        bi_encoder: BiEncoder,
        cross_encoder: CrossEncoder,
        ann_index: NumpyANNIndex | FaissANNIndex,
        scoring: HybridScoringConfig | None = None,
    ):
        self._bi_encoder = bi_encoder
        self._cross_encoder = cross_encoder
        self._ann_index = ann_index
        self._scoring = scoring or HybridScoringConfig()
        self._records: dict[str, ArgumentRecord] = {}

    @classmethod
    def from_huggingface_models(
        cls,
        embedding_model: str = "BAAI/bge-base-en-v1.5",
        cross_encoder_model: str = "cross-encoder/ms-marco-MiniLM-L-12-v2",
        use_faiss: bool = True,
    ) -> "ArgumentRetrievalSubsystem":
        bi_encoder = SentenceTransformerBiEncoder(embedding_model)
        cross_encoder = HFCrossEncoderScorer(cross_encoder_model)

        probe = bi_encoder.encode(["dimension probe"])[0]
        ann_index: NumpyANNIndex | FaissANNIndex
        ann_index = FaissANNIndex(len(probe)) if use_faiss else NumpyANNIndex()
        return cls(bi_encoder=bi_encoder, cross_encoder=cross_encoder, ann_index=ann_index)

    def upsert_arguments(self, arguments: list[ArgumentRecord]) -> None:
        if not arguments:
            return
        ids = [argument.argument_id for argument in arguments]
        vectors = self._bi_encoder.encode([argument.embedding_text for argument in arguments])
        self._ann_index.add(ids, vectors)
        for argument in arguments:
            self._records[argument.argument_id] = argument

    def retrieve(
        self,
        query: RetrievalQuery,
        top_k: int = 10,
        candidate_pool: int = 50,
        filters: RetrievalFilters | None = None,
    ) -> list[RetrievalResult]:
        query_vector = self._bi_encoder.encode([query.text])[0]
        approx = self._ann_index.search(query_vector, max(top_k, candidate_pool))

        filtered: list[tuple[ArgumentRecord, float]] = []
        for argument_id, semantic in approx:
            if argument_id == query.query_argument_id:
                continue
            candidate = self._records.get(argument_id)
            if not candidate or not _passes_filters(candidate, filters):
                continue
            filtered.append((candidate, semantic))

        if not filtered:
            return []

        cross_scores = _normalize_scores(self._cross_encoder.score(query.text, [item[0].embedding_text for item in filtered]))

        ranked: list[RetrievalResult] = []
        for (candidate, semantic), cross in zip(filtered, cross_scores):
            stance_signal = _stance_signal(query.stance, candidate.stance)
            topical_overlap = 1.0 if query.topic_id and query.topic_id == candidate.topic_id else 0.0
            recency_score = _recency_score(query.published_at, candidate.published_at)
            final_score = (
                self._scoring.semantic_weight * semantic
                + self._scoring.cross_encoder_weight * cross
                + self._scoring.stance_weight * stance_signal
                + self._scoring.topical_overlap_weight * topical_overlap
                + self._scoring.recency_weight * recency_score
            )
            ranked.append(
                RetrievalResult(
                    argument=candidate,
                    score=CandidateScore(
                        argument_id=candidate.argument_id,
                        semantic_score=semantic,
                        cross_encoder_score=cross,
                        stance_signal=stance_signal,
                        topical_overlap=topical_overlap,
                        recency_score=recency_score,
                        final_score=final_score,
                    ),
                )
            )

        ranked.sort(key=lambda item: item.score.final_score, reverse=True)
        return ranked[:top_k]


@dataclass(slots=True)
class SimilarArgumentQuery:
    query_argument_id: str
    relevant_argument_ids: set[str]


@dataclass(slots=True)
class BenchmarkReport:
    hit_rate_at_k: float
    mean_reciprocal_rank: float
    evaluated_queries: int


class OfflineRelevanceBenchmark:
    def __init__(self, retrieval: ArgumentRetrievalSubsystem):
        self._retrieval = retrieval

    def run(
        self,
        query_set: list[SimilarArgumentQuery],
        query_lookup: dict[str, RetrievalQuery],
        top_k: int = 10,
    ) -> BenchmarkReport:
        if not query_set:
            return BenchmarkReport(hit_rate_at_k=0.0, mean_reciprocal_rank=0.0, evaluated_queries=0)
        hits = 0
        reciprocal = 0.0
        for item in query_set:
            query = query_lookup[item.query_argument_id]
            ranked_ids = [r.argument.argument_id for r in self._retrieval.retrieve(query, top_k=top_k)]
            rank = next((idx + 1 for idx, arg_id in enumerate(ranked_ids) if arg_id in item.relevant_argument_ids), None)
            if rank:
                hits += 1
                reciprocal += 1.0 / rank
        total = len(query_set)
        return BenchmarkReport(hit_rate_at_k=hits / total, mean_reciprocal_rank=reciprocal / total, evaluated_queries=total)


def argument_to_record(argument: ArgumentUnit, context_text: str, source_type: str, published_at: datetime | None) -> ArgumentRecord:
    return ArgumentRecord(
        argument_id=argument.argument_id,
        speaker_id=argument.speaker_id,
        topic_id=argument.topic_id,
        claim_text=argument.claim_text,
        context_text=context_text,
        stance=argument.stance,
        source_type=source_type,
        published_at=published_at,
    )


def _dot(a: list[float], b: list[float]) -> float:
    return sum(x * y for x, y in zip(a, b))


def _l2_normalize(vector: list[float]) -> list[float]:
    norm = math.sqrt(sum(v * v for v in vector))
    if norm == 0.0:
        return vector
    return [v / norm for v in vector]


def _normalize_scores(scores: list[float]) -> list[float]:
    if not scores:
        return []
    lo = min(scores)
    hi = max(scores)
    if math.isclose(lo, hi):
        return [0.5 for _ in scores]
    return [(score - lo) / (hi - lo) for score in scores]


def _passes_filters(argument: ArgumentRecord, filters: RetrievalFilters | None) -> bool:
    if not filters:
        return True
    if filters.topic_ids and argument.topic_id not in filters.topic_ids:
        return False
    if filters.stances and argument.stance not in filters.stances:
        return False
    if filters.speaker_ids and argument.speaker_id not in filters.speaker_ids:
        return False
    if filters.source_types and argument.source_type not in filters.source_types:
        return False
    if filters.start_date and argument.published_at and argument.published_at < filters.start_date:
        return False
    if filters.end_date and argument.published_at and argument.published_at > filters.end_date:
        return False
    return True


def _stance_signal(query_stance: StanceLabel | None, candidate_stance: StanceLabel) -> float:
    if query_stance is None:
        return 0.5
    if query_stance == candidate_stance:
        return 1.0
    opposite = {StanceLabel.PRO: StanceLabel.CON, StanceLabel.CON: StanceLabel.PRO, StanceLabel.NEUTRAL: StanceLabel.NEUTRAL}
    return 0.75 if opposite[query_stance] == candidate_stance else 0.25


def _recency_score(query_date: datetime | None, candidate_date: datetime | None) -> float:
    if not query_date or not candidate_date:
        return 0.5
    delta_days = abs((query_date - candidate_date).days)
    return max(0.0, 1.0 - min(delta_days, 365) / 365.0)
