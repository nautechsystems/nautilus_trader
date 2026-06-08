#!/usr/bin/env bash
# Check Rekor transparency log availability before sealing a release.
set -euo pipefail

rekor_log_url="${REKOR_LOG_URL:-https://rekor.sigstore.dev/api/v1/log}"
attempts="${REKOR_CHECK_ATTEMPTS:-5}"
retry_delay_seconds="${REKOR_CHECK_RETRY_DELAY_SECONDS:-30}"
curl_connect_timeout="${CURL_CONNECT_TIMEOUT:-20}"
curl_max_time="${CURL_MAX_TIME:-120}"

validate_positive_integer() {
  local name=$1
  local value=$2

  if ! [[ "$value" =~ ^[0-9]+$ ]] || [[ "$value" -lt 1 ]]; then
    echo "::error::${name} must be a positive integer."
    exit 1
  fi
}

validate_positive_integer REKOR_CHECK_ATTEMPTS "$attempts"
validate_positive_integer REKOR_CHECK_RETRY_DELAY_SECONDS "$retry_delay_seconds"
validate_positive_integer CURL_CONNECT_TIMEOUT "$curl_connect_timeout"
validate_positive_integer CURL_MAX_TIME "$curl_max_time"

if ! command -v curl > /dev/null; then
  echo "::error::curl not found."
  exit 1
fi

status=0
delay="$retry_delay_seconds"

for attempt in $(seq 1 "$attempts"); do
  set +e
  curl --proto '=https' --tlsv1.2 --silent --show-error --fail --location \
    --connect-timeout "$curl_connect_timeout" \
    --max-time "$curl_max_time" \
    --output /dev/null \
    "$rekor_log_url"
  status=$?
  set -e

  if [[ "$status" -eq 0 ]]; then
    echo "Rekor transparency log is reachable."
    exit 0
  fi

  if [[ "$attempt" -lt "$attempts" ]]; then
    echo "Rekor readiness check failed (exit=${status}), retry (${attempt}/${attempts}) after ${delay}s"
    sleep "$delay"
    delay=$((delay * 2))
  fi
done

echo "::error::Rekor readiness check failed after ${attempts} attempts."
exit "$status"
