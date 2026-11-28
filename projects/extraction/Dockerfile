FROM python:3.15-slim@sha256:08a4c22c92b79ec821a7ad49768480039ee5b7accac3955228e75366e30abc8e

WORKDIR /workspace

COPY projects/extraction /workspace/projects/extraction
COPY ops /workspace/ops

RUN pip install --no-cache-dir -e /workspace/projects/extraction

CMD ["python", "-m", "mi_extraction.transcription.mock_service"]

