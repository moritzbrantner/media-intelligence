FROM python:3.15-slim@sha256:2fb224844bc0ebfca5f117d1c0b93895652c8ceb01acf2c8c54c3d3a44bdcc7c

WORKDIR /workspace

COPY contracts /workspace/contracts
COPY projects/search /workspace/projects/search

RUN pip install --no-cache-dir -e /workspace/contracts -e /workspace/projects/search

CMD ["python", "-m", "mi_search.main"]

