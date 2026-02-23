from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
import math
from typing import Any, Protocol

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
    lexical_score: float
    semantic_score: float
    cross_encoder_score: float
    stance_signal: float
    topical_overlap: float
    recency_score: float
    final_score: float


@dataclass(slots=True)
class RetrievalExplainability:
    matched_terms: list[str]
    vector_score: float
    lexical_score: float
    applied_filters: dict[str, Any]
    source_type_weights: dict[str, float]
    reranked: bool


@dataclass(slots=True)
class RetrievalResult:
    argument: ArgumentRecord
    score: CandidateScore
    explainability: RetrievalExplainability


@dataclass(slots=True)
class HybridScoringConfig:
    lexical_weight: float = 0.2
    semantic_weight: float = 0.25
    cross_encoder_weight: float = 0.3
    stance_weight: float = 0.1
    topical_overlap_weight: float = 0.1
    recency_weight: float = 0.05


@dataclass(slots=True)
class SourceTypeHybridWeights:
    lexical_weight: float
    semantic_weight: float


DEFAULT_SOURCE_TYPE_WEIGHTS: dict[str, SourceTypeHybridWeights] = {
    "web": SourceTypeHybridWeights(lexical_weight=0.65, semantic_weight=0.35),
    "web_page": SourceTypeHybridWeights(lexical_weight=0.65, semantic_weight=0.35),
    "article": SourceTypeHybridWeights(lexical_weight=0.65, semantic_weight=0.35),
    "transcript": SourceTypeHybridWeights(lexical_weight=0.4, semantic_weight=0.6),
    "podcast": SourceTypeHybridWeights(lexical_weight=0.4, semantic_weight=0.6),
    "video": SourceTypeHybridWeights(lexical_weight=0.4, semantic_weight=0.6),
}
FALLBACK_SOURCE_TYPE_WEIGHTS = SourceTypeHybridWeights(lexical_weight=0.5, semantic_weight=0.5)


@dataclass(slots=True)
class RetrievalQuery:
    query_argument_id: str
    text: str
    topic_id: str | None = None
    stance: StanceLabel | None = None
    published_at: datetime | None = None


@dataclass(slots=True)
class ElasticCandidate:
    argument_id: str
    lexical_score: float
    vector_score: float
    matched_terms: list[str]


@dataclass(slots=True)
class ElasticHybridQuerySpec:
    query: dict[str, Any]


class ElasticSearchClient(Protocol):
    def search(self, index: str, body: dict[str, Any]) -> dict[str, Any]:
        ...


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
        source_type_weights: dict[str, SourceTypeHybridWeights] | None = None,
    ):
        self._bi_encoder = bi_encoder
        self._cross_encoder = cross_encoder
        self._ann_index = ann_index
        self._scoring = scoring or HybridScoringConfig()
        self._source_type_weights = source_type_weights or DEFAULT_SOURCE_TYPE_WEIGHTS
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

    def build_stage_a_query(
        self,
        query: RetrievalQuery,
        *,
        query_embedding: list[float],
        filters: RetrievalFilters | None,
        candidate_pool: int,
        vector_field: str = "chunk_embedding",
    ) -> ElasticHybridQuerySpec:
        bool_filter = _build_elastic_filters(filters)
        fields = ["title^3", "body^2", "description", "text^2", "claim_text^3", "context_text"]
        spec = {
            "size": candidate_pool,
            "track_total_hits": False,
            "query": {
                "bool": {
                    "filter": bool_filter,
                    "should": [
                        {
                            "multi_match": {
                                "query": query.text,
                                "fields": fields,
                                "type": "best_fields",
                            }
                        },
                        {
                            "script_score": {
                                "query": {"match_all": {}},
                                "script": {
                                    "source": f"cosineSimilarity(params.query_vector, '{vector_field}') + 1.0",
                                    "params": {"query_vector": query_embedding},
                                },
                            }
                        },
                    ],
                    "minimum_should_match": 1,
                }
            },
            "highlight": {
                "fields": {
                    "title": {},
                    "body": {},
                    "text": {},
                    "claim_text": {},
                    "context_text": {},
                }
            },
        }
        return ElasticHybridQuerySpec(query=spec)

    def stage_a_candidates(
        self,
        query: RetrievalQuery,
        top_k: int,
        candidate_pool: int,
        filters: RetrievalFilters | None = None,
    ) -> list[ElasticCandidate]:
        query_vector = self._bi_encoder.encode([query.text])[0]
        lexical_scores = self._lexical_scores(query.text)
        semantic_scores = dict(self._ann_index.search(query_vector, max(top_k, candidate_pool)))

        candidates: list[ElasticCandidate] = []
        for argument_id, record in self._records.items():
            if argument_id == query.query_argument_id or not _passes_filters(record, filters):
                continue
            lexical = lexical_scores.get(argument_id, 0.0)
            semantic = semantic_scores.get(argument_id, -1.0)
            if lexical <= 0.0 and semantic <= -1.0:
                continue
            candidates.append(
                ElasticCandidate(
                    argument_id=argument_id,
                    lexical_score=lexical,
                    vector_score=semantic,
                    matched_terms=_matched_terms(query.text, record.claim_text, record.context_text),
                )
            )

        candidates.sort(key=lambda item: item.lexical_score + item.vector_score, reverse=True)
        return candidates[: max(top_k, candidate_pool)]

    def _lexical_scores(self, query_text: str) -> dict[str, float]:
        scores: dict[str, float] = {}
        for argument_id, record in self._records.items():
            overlap = _token_overlap(query_text, record.claim_text, record.context_text)
            scores[argument_id] = overlap
        return scores

    def retrieve(
        self,
        query: RetrievalQuery,
        top_k: int = 10,
        candidate_pool: int = 50,
        filters: RetrievalFilters | None = None,
        rerank: bool = True,
    ) -> list[RetrievalResult]:
        candidates = self.stage_a_candidates(query, top_k=top_k, candidate_pool=candidate_pool, filters=filters)
        if not candidates:
            return []

        candidate_records: list[ArgumentRecord] = []
        for item in candidates:
            record = self._records.get(item.argument_id)
            if record:
                candidate_records.append(record)

        cross_encoder_scores: dict[str, float] = {}
        if rerank:
            normalized_cross_scores = _normalize_scores(
                self._cross_encoder.score(query.text, [item.embedding_text for item in candidate_records])
            )
            cross_encoder_scores = {
                candidate.argument_id: score
                for candidate, score in zip(candidate_records, normalized_cross_scores)
            }

        ranked: list[RetrievalResult] = []
        for candidate in candidates:
            record = self._records.get(candidate.argument_id)
            if not record:
                continue
            source_weights = self._source_type_weights.get(record.source_type, FALLBACK_SOURCE_TYPE_WEIGHTS)
            lexical_semantic_weight = self._scoring.lexical_weight + self._scoring.semantic_weight
            lexical_weight = lexical_semantic_weight * source_weights.lexical_weight
            semantic_weight = lexical_semantic_weight * source_weights.semantic_weight
            cross = cross_encoder_scores.get(record.argument_id, 0.0)
            stance_signal = _stance_signal(query.stance, record.stance)
            topical_overlap = 1.0 if query.topic_id and query.topic_id == record.topic_id else 0.0
            recency_score = _recency_score(query.published_at, record.published_at)
            final_score = (
                lexical_weight * candidate.lexical_score
                + semantic_weight * candidate.vector_score
                + self._scoring.cross_encoder_weight * cross
                + self._scoring.stance_weight * stance_signal
                + self._scoring.topical_overlap_weight * topical_overlap
                + self._scoring.recency_weight * recency_score
            )
            ranked.append(
                RetrievalResult(
                    argument=record,
                    score=CandidateScore(
                        argument_id=record.argument_id,
                        lexical_score=candidate.lexical_score,
                        semantic_score=candidate.vector_score,
                        cross_encoder_score=cross,
                        stance_signal=stance_signal,
                        topical_overlap=topical_overlap,
                        recency_score=recency_score,
                        final_score=final_score,
                    ),
                    explainability=RetrievalExplainability(
                        matched_terms=candidate.matched_terms,
                        vector_score=candidate.vector_score,
                        lexical_score=candidate.lexical_score,
                        applied_filters=_filters_to_dict(filters),
                        source_type_weights={
                            "lexical_weight": source_weights.lexical_weight,
                            "semantic_weight": source_weights.semantic_weight,
                        },
                        reranked=rerank,
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


def _build_elastic_filters(filters: RetrievalFilters | None) -> list[dict[str, Any]]:
    if not filters:
        return []
    clauses: list[dict[str, Any]] = []
    if filters.source_types:
        clauses.append({"terms": {"source_type": sorted(filters.source_types)}})
    if filters.topic_ids:
        clauses.append({"terms": {"topic": sorted(filters.topic_ids)}})
    if filters.stances:
        clauses.append({"terms": {"stance": [stance.value for stance in sorted(filters.stances, key=lambda item: item.value)]}})
    if filters.speaker_ids:
        clauses.append({"terms": {"speaker_id": sorted(filters.speaker_ids)}})
    date_range: dict[str, str] = {}
    if filters.start_date:
        date_range["gte"] = filters.start_date.isoformat()
    if filters.end_date:
        date_range["lte"] = filters.end_date.isoformat()
    if date_range:
        clauses.append({"range": {"published_at": date_range}})
    return clauses


def _filters_to_dict(filters: RetrievalFilters | None) -> dict[str, Any]:
    if not filters:
        return {}
    payload: dict[str, Any] = {}
    if filters.source_types:
        payload["source_type"] = sorted(filters.source_types)
    if filters.topic_ids:
        payload["topic_ids"] = sorted(filters.topic_ids)
    if filters.stances:
        payload["stances"] = [stance.value for stance in sorted(filters.stances, key=lambda item: item.value)]
    if filters.speaker_ids:
        payload["speaker_ids"] = sorted(filters.speaker_ids)
    if filters.start_date or filters.end_date:
        payload["published_at"] = {
            "start_date": filters.start_date.isoformat() if filters.start_date else None,
            "end_date": filters.end_date.isoformat() if filters.end_date else None,
        }
    return payload


def _token_overlap(query_text: str, *texts: str) -> float:
    matches = _matched_terms(query_text, *texts)
    query_terms = {token.strip(".,!?;:\"'()[]{}").lower() for token in query_text.split() if token.strip()}
    if not query_terms:
        return 0.0
    return len(matches) / len(query_terms)


def _matched_terms(query_text: str, *texts: str) -> list[str]:
    query_terms = {token.strip(".,!?;:\"'()[]{}").lower() for token in query_text.split() if token.strip()}
    if not query_terms:
        return []
    corpus_terms: set[str] = set()
    for text in texts:
        corpus_terms.update(token.strip(".,!?;:\"'()[]{}").lower() for token in text.split() if token.strip())
    return sorted(query_terms.intersection(corpus_terms))


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
