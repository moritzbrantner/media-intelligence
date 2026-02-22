#!/usr/bin/env sh
set -eu

required_vars="
REDIS_URL
MINIO_ENDPOINT
MINIO_ACCESS_KEY
MINIO_SECRET_KEY
MINIO_RAW_BUCKET
MINIO_PROCESSED_BUCKET
MINIO_FRAMES_BUCKET
ELASTICSEARCH_URL
"

missing=""
for var in $required_vars; do
  eval "value=\${$var:-}"
  if [ -z "$value" ]; then
    missing="$missing $var"
  fi
done

if [ -n "$missing" ]; then
  echo "[env-validation] Missing required environment variables:$missing" >&2
  echo "[env-validation] Copy .env.example to .env and set values before starting services." >&2
  exit 1
fi

echo "[env-validation] Environment validated for ${SERVICE_NAME:-unknown-service}."
