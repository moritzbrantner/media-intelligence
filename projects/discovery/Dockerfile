FROM python:3.15-slim@sha256:f786b2e19cf9bb54e96bfcd4c1b40c4c56336c22e60607ffd4b080ae5b5dee83

WORKDIR /workspace

COPY contracts /workspace/contracts
COPY projects/discovery /workspace/projects/discovery
COPY ops /workspace/ops

RUN pip install --no-cache-dir -e /workspace/contracts -e /workspace/projects/discovery

CMD ["python", "-m", "mi_discovery.source_watcher.main"]

