FROM python:3.15-slim@sha256:8c13a3f37a6a5b71498228e902c4b9f51c1f7900d49fabf71f9a650c6120e0d0

WORKDIR /workspace

COPY projects/extraction /workspace/projects/extraction
COPY ops /workspace/ops

RUN pip install --no-cache-dir -e /workspace/projects/extraction

CMD ["python", "-m", "mi_extraction.transcription.mock_service"]

