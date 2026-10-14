FROM python:3.15-slim

WORKDIR /workspace

COPY contracts /workspace/contracts
COPY projects/understanding /workspace/projects/understanding
COPY ops /workspace/ops

RUN pip install --no-cache-dir -e /workspace/contracts -e /workspace/projects/understanding

CMD ["python", "-m", "mi_understanding.enrichment.main"]

