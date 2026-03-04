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
BANNED_PATTERN='\bchainsaw\b|maker[_:.\\-]?poc|pocbuspayload|\bpoc\b|\bpoc_[a-z0-9_]+\b|\b[a-z0-9_]+_poc\b'
REDIS_SCHEMA_DOC="docs/flux/redis_schema.md"
ALLOWLIST_START='<!-- leakage-allowlist:start maker_poc_migration -->'
ALLOWLIST_END='<!-- leakage-allowlist:end maker_poc_migration -->'

if rg "${RG_FLAGS[@]}" "$BANNED_PATTERN" "${PRODUCTION_PATHS[@]}"; then
  echo "[flux-leakage] Found forbidden POC/chainsaw naming in production Flux paths." >&2
  exit 1
fi

allowlist_start_count="$(grep -Fxc -- "$ALLOWLIST_START" "$REDIS_SCHEMA_DOC" || true)"
allowlist_end_count="$(grep -Fxc -- "$ALLOWLIST_END" "$REDIS_SCHEMA_DOC" || true)"
if [[ "$allowlist_start_count" != "1" || "$allowlist_end_count" != "1" ]]; then
  echo "[flux-leakage] Expected exactly one redis_schema allowlist start/end marker pair." >&2
  exit 1
fi

allowlist_start_line="$(rg -n -F -- "$ALLOWLIST_START" "$REDIS_SCHEMA_DOC" | cut -d: -f1)"
allowlist_end_line="$(rg -n -F -- "$ALLOWLIST_END" "$REDIS_SCHEMA_DOC" | cut -d: -f1)"
if [[ "$allowlist_start_line" -ge "$allowlist_end_line" ]]; then
  echo "[flux-leakage] redis_schema allowlist markers are out of order." >&2
  exit 1
fi

filtered_redis_schema="$(mktemp)"
cleanup() {
  rm -f "$filtered_redis_schema"
}
trap cleanup EXIT

awk -v start="$ALLOWLIST_START" -v end="$ALLOWLIST_END" '
BEGIN {
  in_allowlist = 0
}
$0 == start {
  in_allowlist = 1
  next
}
$0 == end {
  in_allowlist = 0
  next
}
!in_allowlist {
  print
}
' "$REDIS_SCHEMA_DOC" > "$filtered_redis_schema"

if rg "${RG_FLAGS[@]}" "$BANNED_PATTERN" "$filtered_redis_schema"; then
  echo "[flux-leakage] Found forbidden POC/chainsaw naming in redis_schema outside allowlist markers." >&2
  exit 1
fi

DURABLE_DOCS=(
  docs/flux/redis_schema.md
  docs/flux/params.md
  docs/flux/bridge.md
  docs/flux/api.md
)

ABSOLUTE_PATH_PATTERN='/home/[^/[:space:]]+|/Users/[^/[:space:]]+|(^|[[:space:][:punct:]])[A-Za-z]:(\\|/)'

if rg "${RG_FLAGS[@]}" "$ABSOLUTE_PATH_PATTERN" "${DURABLE_DOCS[@]}"; then
  echo "[flux-leakage] Found absolute host paths in durable Flux docs." >&2
  exit 1
fi

echo "[flux-leakage] OK"
