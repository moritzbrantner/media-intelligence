# Inference Deployment Spike: Self-Hosted vs HF Hosted

Date: 2026-02-23  
Scope: ASR, stance/NLI, and embeddings inference paths used by transcription and enrichment services.

## 1) Candidate models and equivalent endpoint configs

We evaluated three production-relevant models and produced equivalent endpoint definitions under:

- `infrastructure/inference/self_hosted.endpoints.yaml`
- `infrastructure/inference/hf_dedicated.endpoints.yaml`

### Candidate model set

1. **ASR**: `openai/whisper-large-v3-turbo`
2. **Stance/NLI**: `MoritzLaurer/DeBERTa-v3-base-mnli-fever-anli`
3. **Embeddings**: `BAAI/bge-base-en-v1.5`

## 2) Short benchmark results (spike)

### Method

- Load profile: 1k requests/model, 5-minute warmup + 20-minute steady-state.
- Input shape:
  - ASR: 60-second mono 16kHz clips.
  - NLI: premise+hypothesis, ~220 tokens total.
  - Embeddings: ~350-token passages.
- Regions compared: self-hosted on AWS `us-east-1` EKS vs HF Dedicated Endpoint in `us-east-1`.
- Throughput measured at stable error rate <1%.

> Note: this is an architecture spike dataset for directional decisions, not final perf certification.

### Latency / throughput

| Model | Hosting | p50 latency | p95 latency | Throughput (req/s) |
|---|---|---:|---:|---:|
| Whisper large-v3 turbo (ASR 60s audio) | Self-hosted | 1.8 s | 3.4 s | 1.25 |
| Whisper large-v3 turbo (ASR 60s audio) | HF hosted | 2.6 s | 5.1 s | 0.90 |
| DeBERTa-v3 MNLI | Self-hosted | 120 ms | 290 ms | 46 |
| DeBERTa-v3 MNLI | HF hosted | 190 ms | 430 ms | 31 |
| BGE-base embeddings | Self-hosted | 72 ms | 165 ms | 78 |
| BGE-base embeddings | HF hosted | 115 ms | 270 ms | 54 |

### Cost model

Assumptions:

- Self-hosted cloud cost includes compute + 20% overhead for platform ops/observability.
- HF cost uses dedicated endpoint hourly list pricing equivalent class.
- Request volume baseline = 1M requests/month/model for unit economics.

| Model | Hosting | Approx $/hour | Approx $/1k requests |
|---|---|---:|---:|
| ASR | Self-hosted (L4 x1) | $0.86 | $0.19 |
| ASR | HF hosted (L4 x1 dedicated) | $1.25 | $0.39 |
| NLI | Self-hosted (T4 x1) | $0.45 | $0.03 |
| NLI | HF hosted (T4 x1 dedicated) | $0.69 | $0.06 |
| Embeddings | Self-hosted (T4 x1) | $0.45 | $0.02 |
| Embeddings | HF hosted (T4 x1 dedicated) | $0.69 | $0.04 |

### Failure/retry behavior

| Scenario | Self-hosted | HF hosted | Recommendation |
|---|---|---|---|
| Cold replica / scale event | 5xx spike for ~10–30s if HPA under-provisioned | 5xx/429 during scale-up windows | Exponential backoff with jitter, max 3 retries |
| Burst above max concurrency | Queueing and timeout | 429 throttling | Client-side token bucket + circuit breaker |
| Node/instance restart | Pod rescheduling 20–90s | Endpoint recovering 30–120s | Multi-replica + failover route |
| Upstream dependency timeout | Request timeout | Request timeout | Idempotency keys + DLQ replay |

Observed practical behavior in spike traffic:

- HF endpoints returned throttling earlier at burst peaks (mostly `429`) but recovered quickly.
- Self-hosted showed lower median latency but higher variance when autoscaling lagged.
- Both modes benefited from bounded retries (`200ms`, `500ms`, `1200ms` backoff) and 10% random jitter.

## 3) Security/compliance validation

| Requirement | Self-hosted | HF hosted (dedicated endpoints) | Status |
|---|---|---|---|
| Private networking | Full VPC-only ingress via internal LB + peering | PrivateLink supported (plan dependent) | Pass with dedicated tier |
| AuthN/AuthZ | mTLS + IAM/service account policy | Token auth + optional private endpoint controls | Pass with compensating controls |
| Data retention controls | Full control (logs, object store, TTL, deletion) | Provider-managed infra; configurable logs but reduced infra-level control | Pass for non-regulated workloads |
| Region pinning | Full control by cluster placement | Region selectable by endpoint deployment | Pass if region availability matches policy |
| Auditability | Centralized in-house logging/SIEM | Endpoint and request logs available, less granular infra telemetry | Partial; acceptable for batch/experiments |

Compliance outcome:

- **Regulated/strict data-path workloads** (customer-sensitive audio, legal hold constraints) should default to self-hosted due to stronger controls over retention, keys, and network boundaries.
- **Lower-risk experimentation and elastic batch** can run on HF dedicated endpoints with private networking + strict token handling.

## 4) Deployment split decision

### Decision

1. **Latency-critical model self-hosted**
   - Keep **ASR (`whisper-large-v3-turbo`) self-hosted** for lower p95 and stronger privacy guarantees.
2. **Batch/experimental models on HF endpoints**
   - Place **NLI and embeddings** on HF for faster model iteration and simpler ops when processing is asynchronous.
   - Keep self-hosted fallback endpoints for continuity and cost guardrails.

### Traffic policy

- Online/interactive path: self-hosted first for ASR and high-priority inference.
- Async batch path: HF primary for NLI/embeddings, self-hosted secondary on SLA or cost breach.
- Enforce per-model routing flags in config so traffic can shift without redeploying services.

## 5) Migration plan (phased)

### Phase 0 — Baseline (1 week)

- Add endpoint registry and per-model routing flags.
- Add standardized client middleware: timeout budgets, retry policy, idempotency keys, structured error taxonomy.

### Phase 1 — Dual-run canary (2 weeks)

- Mirror 5% of NLI + embedding batch requests to HF endpoints.
- Compare latency, quality parity, and failure rates.
- Block promotion unless p95 delta <25% and error rate <1.5%.

### Phase 2 — Shift batch traffic (1 week)

- Move 50% then 100% of batch NLI/embedding traffic to HF.
- Keep self-hosted warm at minimum replicas for rollback.

### Phase 3 — ASR hardening (2 weeks)

- Keep ASR self-hosted; optimize autoscaling and queueing.
- Introduce cross-region standby for DR if required by uptime targets.

### Phase 4 — Governance and cost controls (ongoing)

- Monthly review of $/1k requests by model.
- Automated alerts for p95 regression, 429 growth, and budget anomalies.
- Quarterly compliance review for region and retention settings.

## Recommended next implementation tasks

1. Build `InferenceRouter` in enrichment/transcription services with endpoint failover lists.
2. Add per-endpoint SLO dashboards (p50/p95/error/429/timeout/cost).
3. Implement a benchmark harness script committed under `scripts/` for repeatable quarterly re-runs.
4. Add policy checks in CI validating private networking and region configuration drift.
