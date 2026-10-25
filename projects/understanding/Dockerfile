FROM python:3.15-slim@sha256:533b0b2f9b5d46fd097091cf826f27902f1d6c3bca714b6dc74551356aa97a51

WORKDIR /workspace

COPY contracts /workspace/contracts
COPY projects/understanding /workspace/projects/understanding
COPY ops /workspace/ops

RUN pip install --no-cache-dir -e /workspace/contracts -e /workspace/projects/understanding

CMD ["python", "-m", "mi_understanding.enrichment.main"]

