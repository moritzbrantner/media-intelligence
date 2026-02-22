# Infrastructure

## Overview

The platform runs on Docker Compose for local development and Kubernetes for production. All services are stateless; state lives in Redis (queues), MinIO (object storage), and Elasticsearch (search/profiles).

---

## Local Development Stack

`docker-compose.yml` should define these services:

```yaml
version: "3.9"
services:

  redis:
    image: redis:7-alpine
    ports: ["6379:6379"]

  minio:
    image: minio/minio
    command: server /data --console-address ":9001"
    environment:
      MINIO_ROOT_USER: minioadmin
      MINIO_ROOT_PASSWORD: minioadmin
    ports:
      - "9000:9000"   # API
      - "9001:9001"   # Console UI
    volumes:
      - minio_data:/data

  elasticsearch:
    image: docker.elastic.co/elasticsearch/elasticsearch:8.12.0
    environment:
      - discovery.type=single-node
      - xpack.security.enabled=false
      - "ES_JAVA_OPTS=-Xms1g -Xmx1g"
    ports: ["9200:9200"]
    volumes:
      - es_data:/usr/share/elasticsearch/data

  kibana:
    image: docker.elastic.co/kibana/kibana:8.12.0
    environment:
      ELASTICSEARCH_HOSTS: http://elasticsearch:9200
    ports: ["5601:5601"]
    depends_on: [elasticsearch]

  # Add one container per service:
  # ingestion, transcription, ocr, enrichment, profiling, indexer
  # Each reads from env vars and connects to redis/minio/elasticsearch

volumes:
  minio_data:
  es_data:
```

---

## MinIO Bucket Setup

Run once on first startup:

```bash
# Using mc (MinIO client)
mc alias set local http://localhost:9000 minioadmin minioadmin
mc mb local/raw-media
mc mb local/processed
mc mb local/frames
```

Or use the MinIO Python client in a setup script:

```python
from minio import Minio

client = Minio("localhost:9000", access_key="minioadmin", secret_key="minioadmin", secure=False)
for bucket in ["raw-media", "processed", "frames"]:
    if not client.bucket_exists(bucket):
        client.make_bucket(bucket)
```

---

## Redis Queue Configuration

Each Celery worker connects to Redis. Queue bindings:

| Queue Name | Workers |
|---|---|
| `ingestion.jobs` | ingestion worker |
| `media.ingested` | transcription worker, ocr worker |
| `content.extracted` | enrichment worker (waits for both transcript + OCR) |
| `content.enriched` | indexer worker, profiling worker |
| `dead.letter` | monitoring / alerting |

**Content coordination**: The enrichment worker should wait until both transcript and OCR results exist before running. Use a lightweight coordinator: when `content.extracted` is published, check if the peer output also exists in MinIO. If not, re-queue with a 60-second delay (up to 5 retries).

---

## Elasticsearch Configuration

For development, security is disabled (see docker-compose above). For production:
- Enable `xpack.security.enabled=true`
- Create a dedicated API key per service with minimal permissions
- Enable TLS

**Recommended Elasticsearch settings** for this workload:

```json
PUT /media_segments/_settings
{
  "index": {
    "number_of_shards": 3,
    "number_of_replicas": 1,
    "refresh_interval": "30s"
  }
}
```

The `refresh_interval: 30s` reduces indexing overhead compared to the default 1s.

---

## Production: Kubernetes

In production, each service runs as a Kubernetes `Deployment` with:
- A `ConfigMap` for non-secret configuration
- A `Secret` for credentials
- Horizontal Pod Autoscaling based on queue depth (via KEDA — Kubernetes Event-Driven Autoscaling)

**KEDA ScaledObject example** (scale transcription workers based on queue depth):

```yaml
apiVersion: keda.sh/v1alpha1
kind: ScaledObject
metadata:
  name: transcription-worker-scaler
spec:
  scaleTargetRef:
    name: transcription-worker
  minReplicaCount: 1
  maxReplicaCount: 10
  triggers:
    - type: redis
      metadata:
        address: redis:6379
        listName: media.ingested
        listLength: "5"
```

**GPU node pools**: Transcription and Enrichment (embedding) workers should target GPU nodes via `nodeSelector` or `tolerations`.

---

## Secrets Management

In production, use Kubernetes Secrets or HashiCorp Vault. Never commit credentials to the repository.

Required secrets:
- `MINIO_ACCESS_KEY` / `MINIO_SECRET_KEY`
- `ELASTICSEARCH_API_KEY`
- `HF_TOKEN` (HuggingFace, for pyannote)
- `TWITTER_BEARER_TOKEN`

---

## Monitoring & Observability

- **Logs**: All services use Python `logging` with JSON output format. Collect with Filebeat → Elasticsearch → Kibana
- **Metrics**: Expose Prometheus metrics from each worker (queue depth, processing time, error rate)
- **Dead letter monitoring**: Alert when `dead.letter` queue depth exceeds 10 messages
- **Kibana dashboards**: 
  - Pipeline throughput (documents/hour)
  - Processing time per stage
  - Error rate per service
  - Entity mention trends

---

## Hardware Recommendations

| Service | CPU | RAM | GPU | Notes |
|---|---|---|---|---|
| Ingestion | 2 vCPU | 4GB | No | I/O bound |
| Transcription | 4 vCPU | 16GB | Yes (CUDA) | Most GPU-intensive |
| OCR | 4 vCPU | 8GB | Optional | CPU-parallelizable |
| Enrichment | 4 vCPU | 16GB | Optional (for embeddings) | |
| Profiling | 2 vCPU | 4GB | No | Mostly ES writes |
| Indexer | 2 vCPU | 4GB | No | Mostly ES writes |
| Elasticsearch | 8 vCPU | 32GB | No | Scale with data volume |
| MinIO | 4 vCPU | 8GB | No | Scale with storage |

For a development machine, a single node with 32GB RAM and an NVIDIA GPU (8GB+ VRAM) can run everything.
