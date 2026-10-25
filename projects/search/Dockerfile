FROM python:3.15-slim@sha256:533b0b2f9b5d46fd097091cf826f27902f1d6c3bca714b6dc74551356aa97a51

WORKDIR /workspace

COPY contracts /workspace/contracts
COPY projects/search /workspace/projects/search

RUN pip install --no-cache-dir -e /workspace/contracts -e /workspace/projects/search

CMD ["python", "-m", "mi_search.main"]

