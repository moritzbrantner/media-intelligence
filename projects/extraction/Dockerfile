FROM python:3.15-slim@sha256:e97b88e416c3c01eb427b21fd8cd3ebf006e6ee38afcfcc3cae1c54ee2ab0a75

WORKDIR /workspace

COPY projects/extraction /workspace/projects/extraction
COPY ops /workspace/ops

RUN pip install --no-cache-dir -e /workspace/projects/extraction

CMD ["python", "-m", "mi_extraction.transcription.mock_service"]

