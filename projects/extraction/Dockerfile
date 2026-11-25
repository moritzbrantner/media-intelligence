FROM python:3.15-slim@sha256:63bb298d2b7b88f176f51b5189f54ce6535c997682ea70885d2132f5d473a8a5

WORKDIR /workspace

COPY projects/extraction /workspace/projects/extraction
COPY ops /workspace/ops

RUN pip install --no-cache-dir -e /workspace/projects/extraction

CMD ["python", "-m", "mi_extraction.transcription.mock_service"]

