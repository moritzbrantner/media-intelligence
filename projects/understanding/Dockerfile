FROM python:3.15-slim@sha256:490c3463f3766091067a01528e6be3208104eebcca11a306beab5de9672ee893

WORKDIR /workspace

COPY contracts /workspace/contracts
COPY projects/understanding /workspace/projects/understanding
COPY ops /workspace/ops

RUN pip install --no-cache-dir -e /workspace/contracts -e /workspace/projects/understanding

CMD ["python", "-m", "mi_understanding.enrichment.main"]

