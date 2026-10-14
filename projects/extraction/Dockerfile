FROM python:3.15-slim

WORKDIR /workspace

COPY projects/extraction /workspace/projects/extraction
COPY ops /workspace/ops

RUN pip install --no-cache-dir -e /workspace/projects/extraction

CMD ["python", "-m", "mi_extraction.transcription.mock_service"]

