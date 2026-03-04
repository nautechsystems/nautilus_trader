#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel)"
cd "$ROOT_DIR"

PRODUCTION_PATHS=(
  nautilus_trader/flux
  docs/flux/params.md
  docs/flux/bridge.md
  docs/flux/api.md
)

RG_FLAGS=(-n -S -i)
BANNED_PATTERN='\bchainsaw\b|maker[_:.\\-]?poc|pocbuspayload|\bpoc\b'

if rg "${RG_FLAGS[@]}" "$BANNED_PATTERN" "${PRODUCTION_PATHS[@]}"; then
  echo "[flux-leakage] Found forbidden POC/chainsaw naming in production Flux paths." >&2
  exit 1
fi

DURABLE_DOCS=(
  docs/flux/redis_schema.md
  docs/flux/params.md
  docs/flux/bridge.md
  docs/flux/api.md
)

ABSOLUTE_PATH_PATTERN='/home/ubuntu|/Users/|[A-Za-z]:\\'

if rg "${RG_FLAGS[@]}" "$ABSOLUTE_PATH_PATTERN" "${DURABLE_DOCS[@]}"; then
  echo "[flux-leakage] Found absolute host paths in durable Flux docs." >&2
  exit 1
fi

echo "[flux-leakage] OK"
