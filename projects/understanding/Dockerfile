FROM python:3.15-slim@sha256:8c13a3f37a6a5b71498228e902c4b9f51c1f7900d49fabf71f9a650c6120e0d0

WORKDIR /workspace

COPY contracts /workspace/contracts
COPY projects/understanding /workspace/projects/understanding
COPY ops /workspace/ops

RUN pip install --no-cache-dir -e /workspace/contracts -e /workspace/projects/understanding

CMD ["python", "-m", "mi_understanding.enrichment.main"]

