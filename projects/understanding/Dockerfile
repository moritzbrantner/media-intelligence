FROM python:3.15-slim@sha256:65a65fc7b30b06cb6df4930fd4650697cb23f4c2698ab81fc87e4b031bc36928

WORKDIR /workspace

COPY contracts /workspace/contracts
COPY projects/understanding /workspace/projects/understanding
COPY ops /workspace/ops

RUN pip install --no-cache-dir -e /workspace/contracts -e /workspace/projects/understanding

CMD ["python", "-m", "mi_understanding.enrichment.main"]

