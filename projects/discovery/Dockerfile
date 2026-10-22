FROM python:3.15-slim@sha256:044d5bd747976e21f8bc4d746c3479b19a84bb8ae6261b802acdd26b29036c93

WORKDIR /workspace

COPY contracts /workspace/contracts
COPY projects/discovery /workspace/projects/discovery
COPY ops /workspace/ops

RUN pip install --no-cache-dir -e /workspace/contracts -e /workspace/projects/discovery

CMD ["python", "-m", "mi_discovery.source_watcher.main"]

