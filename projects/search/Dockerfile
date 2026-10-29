FROM python:3.15-slim@sha256:94d3426f1def7db4a106a232ac121bfd975393e5dfd40a619b955fae45fe7913

WORKDIR /workspace

COPY contracts /workspace/contracts
COPY projects/search /workspace/projects/search

RUN pip install --no-cache-dir -e /workspace/contracts -e /workspace/projects/search

CMD ["python", "-m", "mi_search.main"]

