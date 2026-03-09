#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel)"
cd "$ROOT_DIR"

PRODUCTION_PATHS=(
  systems/flux/flux
  systems/flux/docs/redis_schema.md
  systems/flux/docs/params.md
  systems/flux/docs/bridge.md
  systems/flux/docs/api.md
  apps/fluxboard/docs/tokenmm_contract.md
  apps/fluxboard/docs/tokenmm_socket_contract.md
  apps/fluxboard/docs/tokenmm_runbook.md
)

RG_FLAGS=(-n -S -i)
BANNED_PATTERN='\bchainsaw\b|maker[_:.\\-]?poc|pocbuspayload|\bpoc\b|\bpoc_[a-z0-9_]+\b|\b[a-z0-9_]+_poc\b'

existing_production_paths=()
for path in "${PRODUCTION_PATHS[@]}"; do
  if [[ -e "$path" || -L "$path" ]]; then
    existing_production_paths+=("$path")
  fi
done

if (( ${#existing_production_paths[@]} > 0 )) && rg "${RG_FLAGS[@]}" "$BANNED_PATTERN" "${existing_production_paths[@]}"; then
  echo "[flux-leakage] Found forbidden POC/chainsaw naming in production Flux paths." >&2
  exit 1
fi

DURABLE_DOCS=(
  systems/flux/docs/redis_schema.md
  systems/flux/docs/params.md
  systems/flux/docs/bridge.md
  systems/flux/docs/api.md
  apps/fluxboard/docs/tokenmm_contract.md
  apps/fluxboard/docs/tokenmm_socket_contract.md
  apps/fluxboard/docs/tokenmm_runbook.md
)

ABSOLUTE_PATH_PATTERN='/home/[^/[:space:]]+|/Users/[^/[:space:]]+|(^|[[:space:][:punct:]])[A-Za-z]:(\\|/)'

existing_durable_docs=()
for path in "${DURABLE_DOCS[@]}"; do
  if [[ -e "$path" || -L "$path" ]]; then
    existing_durable_docs+=("$path")
  fi
done

if (( ${#existing_durable_docs[@]} > 0 )) && rg "${RG_FLAGS[@]}" "$ABSOLUTE_PATH_PATTERN" "${existing_durable_docs[@]}"; then
  echo "[flux-leakage] Found absolute host paths in durable Flux docs." >&2
  exit 1
fi

echo "[flux-leakage] OK"
