#!/usr/bin/env bash
set -euo pipefail

BASE_URL="http://127.0.0.1:5022"

usage() {
  cat <<'USAGE'
Usage: ops/scripts/deploy/check_lp_rollout.sh [--base-url URL]

Verify the LP production rollout contract against the shared public host.
USAGE
}

fail() {
  echo "[lp-rollout] $1" >&2
  exit 1
}

check_html() {
  local path="$1"
  local body
  body="$(mktemp)"
  local code
  code="$(curl -sS -o "$body" -w '%{http_code}' "${BASE_URL}${path}")" || {
    rm -f "$body"
    fail "failed to reach ${path}"
  }
  [[ "$code" == "200" ]] || {
    rm -f "$body"
    fail "${path} returned HTTP ${code}"
  }
  grep -Eqi '<!doctype html|<html' "$body" || {
    rm -f "$body"
    fail "${path} did not return HTML"
  }
  rm -f "$body"
}

check_json_ok() {
  local path="$1"
  local body
  body="$(curl -fsS "${BASE_URL}${path}")" || fail "failed to reach ${path}"
  printf '%s' "$body" | python3 -c '
import json
import sys

payload = json.load(sys.stdin)
if payload.get("ok") is not True:
    raise SystemExit(1)
' || fail "${path} did not return ok=true JSON"
}

check_pulse_jobs() {
  local path="$1"
  local body
  body="$(curl -fsS "${BASE_URL}${path}")" || fail "failed to reach ${path}"
  printf '%s' "$body" | python3 -c '
import json
import sys

payload = json.load(sys.stdin)
required = {"jobs", "total", "active", "failed"}
if not isinstance(payload, dict):
    raise SystemExit(1)
if missing := sorted(required - set(payload)):
    raise SystemExit(1)
if not isinstance(payload["jobs"], list):
    raise SystemExit(1)
' || fail "${path} did not return the expected Pulse jobs JSON"
}

main() {
  while (($# > 0)); do
    case "$1" in
      --base-url)
        (($# >= 2)) || fail "--base-url requires a value"
        BASE_URL="${2%/}"
        shift 2
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        usage >&2
        fail "unknown argument: $1"
        ;;
    esac
  done

  check_html "/lp"
  check_json_ok "/api/v1/hedgers/instances"
  check_json_ok "/api/v1/hedgers/eth_plume_lp"
  check_pulse_jobs "/api/pulse/jobs"
  echo "[lp-rollout] rollout checks passed against ${BASE_URL}"
}

main "$@"
