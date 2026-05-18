#!/usr/bin/env bash
# Check that every crates.io-publishable workspace crate enforces Trusted Publishing Only.
#
# Optional env:
#   CRATES_IO_API_URL              - crates.io API base URL (default: https://crates.io/api/v1)
#   CRATES_IO_CHECK_DELAY_SECONDS  - delay between crates.io API requests (default: 1)
#   CRATES_IO_CHECK_USER_AGENT     - crates.io API User-Agent header
set -euo pipefail

crates_io_api_url="${CRATES_IO_API_URL:-https://crates.io/api/v1}"
crates_io_check_delay_seconds="${CRATES_IO_CHECK_DELAY_SECONDS:-1}"
curl_retries="${CURL_RETRIES:-5}"
curl_connect_timeout="${CURL_CONNECT_TIMEOUT:-20}"
curl_max_time="${CURL_MAX_TIME:-300}"

github_url="${GITHUB_SERVER_URL:-https://github.com}"
github_repository="${GITHUB_REPOSITORY:-nautechsystems/nautilus_trader}"
github_run_id="${GITHUB_RUN_ID:-local}"
default_user_agent="nautilus-trader-ci (${github_url}/${github_repository}/actions/runs/${github_run_id})"
crates_io_check_user_agent="${CRATES_IO_CHECK_USER_AGENT:-$default_user_agent}"

if ! command -v cargo > /dev/null; then
  echo "::error::cargo not found."
  exit 1
fi
if ! command -v curl > /dev/null; then
  echo "::error::curl not found."
  exit 1
fi
if ! command -v jq > /dev/null; then
  echo "::error::jq not found."
  exit 1
fi

validate_non_negative_integer() {
  local name=$1
  local value=$2

  if ! [[ "$value" =~ ^[0-9]+$ ]]; then
    echo "::error::${name} must be a non-negative integer."
    exit 1
  fi
}

validate_positive_integer() {
  local name=$1
  local value=$2

  if ! [[ "$value" =~ ^[0-9]+$ ]] || [[ "$value" -lt 1 ]]; then
    echo "::error::${name} must be a positive integer."
    exit 1
  fi
}

validate_non_negative_integer CRATES_IO_CHECK_DELAY_SECONDS "$crates_io_check_delay_seconds"
validate_positive_integer CURL_RETRIES "$curl_retries"
validate_positive_integer CURL_CONNECT_TIMEOUT "$curl_connect_timeout"
validate_positive_integer CURL_MAX_TIME "$curl_max_time"

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

crates_file="${work_dir}/publishable-crates.txt"
response_file="${work_dir}/crate.json"

cargo metadata --no-deps --format-version=1 |
  jq -r '
	  .packages[]
	  | select(.source == null and (.publish == null or (.publish | index("crates-io"))))
	  | .name
	' |
  sort > "$crates_file"

if [[ ! -s "$crates_file" ]]; then
  echo "::error::No crates.io-publishable workspace crates found."
  exit 1
fi

failed=false
checked=0

while IFS= read -r crate_name; do
  : > "$response_file"

  http_status="$(
    curl --proto '=https' --tlsv1.2 --silent --show-error --location \
      --retry "$curl_retries" \
      --retry-all-errors \
      --connect-timeout "$curl_connect_timeout" \
      --max-time "$curl_max_time" \
      --header "User-Agent: ${crates_io_check_user_agent}" \
      --output "$response_file" \
      --write-out '%{http_code}' \
      "${crates_io_api_url%/}/crates/${crate_name}"
  )"

  checked=$((checked + 1))

  if [[ "$http_status" != "200" ]]; then
    echo "::error::${crate_name}: crates.io lookup failed (HTTP ${http_status})."
    cat "$response_file"
    echo
    failed=true
  elif [[ "$(jq -r '.crate.trustpub_only // false' "$response_file")" != "true" ]]; then
    echo "::error::${crate_name}: trustpub_only is not true."
    failed=true
  else
    printf '%-34s trustpub_only=true\n' "$crate_name"
  fi

  if [[ "$crates_io_check_delay_seconds" -gt 0 ]]; then
    sleep "$crates_io_check_delay_seconds"
  fi
done < "$crates_file"

if [[ "$failed" == true ]]; then
  echo "::error::Trusted Publishing Only check failed for one or more crates."
  exit 1
fi

echo "Trusted Publishing Only enabled for ${checked} crates.io-publishable workspace crates."
