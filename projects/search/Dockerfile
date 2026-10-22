FROM python:3.15-slim@sha256:490c3463f3766091067a01528e6be3208104eebcca11a306beab5de9672ee893

WORKDIR /workspace

COPY contracts /workspace/contracts
COPY projects/search /workspace/projects/search

RUN pip install --no-cache-dir -e /workspace/contracts -e /workspace/projects/search

CMD ["python", "-m", "mi_search.main"]

