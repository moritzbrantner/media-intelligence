FROM python:3.15-slim@sha256:e97b88e416c3c01eb427b21fd8cd3ebf006e6ee38afcfcc3cae1c54ee2ab0a75

WORKDIR /workspace

COPY contracts /workspace/contracts
COPY projects/search /workspace/projects/search

RUN pip install --no-cache-dir -e /workspace/contracts -e /workspace/projects/search

CMD ["python", "-m", "mi_search.main"]

