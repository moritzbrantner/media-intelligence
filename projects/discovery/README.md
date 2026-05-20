# `mi_discovery`

Discovery bounded context for ingestion-adjacent services.

## Install

```bash
pip install -e ../../contracts -e .
```

## Test

```bash
python -m pytest
```

## Run

```bash
python -m mi_discovery.source_watcher.main
python -m mi_discovery.ingestion.main
```

