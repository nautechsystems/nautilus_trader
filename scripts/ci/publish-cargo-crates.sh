#!/usr/bin/env bash
# Publish workspace crates to crates.io one at a time in dependency order.
#
# Usage:
#   publish-cargo-crates.sh [--check]
#
# Required env for publishing:
#   CARGO_REGISTRY_TOKEN - crates.io token, preferably from trusted publishing
#
# Optional env:
#   CARGO_PUBLISH_ATTEMPTS             - cargo publish attempts per crate (default: 3)
#   CARGO_PUBLISH_DELAY_SECONDS        - default delay for polling, retries, and success sleeps
#   CARGO_PUBLISH_POLL_SECONDS         - delay between visibility polls
#   CARGO_PUBLISH_RETRY_DELAY_SECONDS  - retry delay floor after cargo publish failures
#   CARGO_PUBLISH_SUCCESS_DELAY_SECONDS - delay after a successful publish
#   CARGO_PUBLISH_WAIT_TIMEOUT_SECONDS - visibility wait per crate (default: 300)
#   CARGO_REGISTRY_API_URL             - crates.io API base URL (default: https://crates.io/api/v1)
#   CARGO_SPARSE_INDEX_URL             - crates.io sparse index URL (default: https://index.crates.io)
#   CARGO_PUBLISH_USER_AGENT           - crates.io API User-Agent header
set -euo pipefail

check_only=false
if [[ "${1:-}" == "--check" ]]; then
  check_only=true
  shift
fi

if [[ "$#" -ne 0 ]]; then
  echo "Usage: $0 [--check]" >&2
  exit 1
fi

cargo_publish_attempts="${CARGO_PUBLISH_ATTEMPTS:-3}"
cargo_publish_delay_seconds="${CARGO_PUBLISH_DELAY_SECONDS:-30}"
cargo_publish_poll_seconds="${CARGO_PUBLISH_POLL_SECONDS:-$cargo_publish_delay_seconds}"
cargo_publish_retry_delay_seconds="${CARGO_PUBLISH_RETRY_DELAY_SECONDS:-$cargo_publish_delay_seconds}"
cargo_publish_success_delay_seconds="${CARGO_PUBLISH_SUCCESS_DELAY_SECONDS:-$cargo_publish_delay_seconds}"
cargo_publish_wait_timeout_seconds="${CARGO_PUBLISH_WAIT_TIMEOUT_SECONDS:-300}"
cargo_registry_api_url="${CARGO_REGISTRY_API_URL:-https://crates.io/api/v1}"
cargo_sparse_index_url="${CARGO_SPARSE_INDEX_URL:-https://index.crates.io}"
curl_retries="${CURL_RETRIES:-5}"
curl_connect_timeout="${CURL_CONNECT_TIMEOUT:-20}"
curl_max_time="${CURL_MAX_TIME:-300}"

github_url="${GITHUB_SERVER_URL:-https://github.com}"
github_repository="${GITHUB_REPOSITORY:-nautechsystems/nautilus_trader}"
github_run_id="${GITHUB_RUN_ID:-local}"
default_user_agent="nautilus-trader-ci (${github_url}/${github_repository}/actions/runs/${github_run_id})"
cargo_publish_user_agent="${CARGO_PUBLISH_USER_AGENT:-$default_user_agent}"

if [[ "$check_only" == false && -z "${CARGO_REGISTRY_TOKEN:-}" ]]; then
  echo "::error::CARGO_REGISTRY_TOKEN not set."
  exit 1
fi

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

validate_positive_integer() {
  local name=$1
  local value=$2

  if ! [[ "$value" =~ ^[0-9]+$ ]] || [[ "$value" -lt 1 ]]; then
    echo "::error::${name} must be a positive integer."
    exit 1
  fi
}

validate_non_negative_integer() {
  local name=$1
  local value=$2

  if ! [[ "$value" =~ ^[0-9]+$ ]]; then
    echo "::error::${name} must be a non-negative integer."
    exit 1
  fi
}

validate_positive_integer CARGO_PUBLISH_ATTEMPTS "$cargo_publish_attempts"
validate_positive_integer CARGO_PUBLISH_DELAY_SECONDS "$cargo_publish_delay_seconds"
validate_positive_integer CARGO_PUBLISH_POLL_SECONDS "$cargo_publish_poll_seconds"
validate_positive_integer CARGO_PUBLISH_RETRY_DELAY_SECONDS "$cargo_publish_retry_delay_seconds"
validate_positive_integer CARGO_PUBLISH_WAIT_TIMEOUT_SECONDS "$cargo_publish_wait_timeout_seconds"
validate_positive_integer CURL_RETRIES "$curl_retries"
validate_positive_integer CURL_CONNECT_TIMEOUT "$curl_connect_timeout"
validate_positive_integer CURL_MAX_TIME "$curl_max_time"
validate_non_negative_integer CARGO_PUBLISH_SUCCESS_DELAY_SECONDS "$cargo_publish_success_delay_seconds"

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

metadata_file="${work_dir}/metadata.json"
publish_plan_file="${work_dir}/publish-plan.tsv"
blocked_dependencies_file="${work_dir}/blocked-dependencies.tsv"
response_file="${work_dir}/response.json"
index_response_file="${work_dir}/sparse-index.json"

cargo metadata --no-deps --format-version=1 > "$metadata_file"

jq -r '
  def crates_io_publishable:
    .publish == null or (.publish | index("crates-io"));

  [.packages[]
    | select(.source == null and crates_io_publishable)
    | {
        name,
        version,
        deps: ([.dependencies[]
          | select(.path != null and (.kind != "dev"))
          | .name
        ] | unique)
      }
  ] as $packages
  | ($packages | map(.name)) as $names
  | def emit($done):
      if ($done | length) == ($packages | length) then empty
      else
        [ $packages[]
          | select(.name as $name | ($done | index($name) | not))
          | select(((.deps | map(. as $dep | select($names | index($dep)))) - $done | length) == 0)
        ] as $ready
        | if ($ready | length) == 0 then
            [ $packages[]
              | select(.name as $name | ($done | index($name) | not))
              | .name + " waits for "
                + ((.deps | map(. as $dep | select($names | index($dep)) | select($done | index(.) | not)))
                  | join(", "))
            ] as $remaining
            | error("unable to sort publishable crates: " + ($remaining | join("; ")))
          else
            ($ready[] | [.name, .version] | @tsv),
            emit($done + ($ready | map(.name)))
          end
      end;
  emit([])
' "$metadata_file" > "$publish_plan_file"

jq -r '
  def crates_io_publishable:
    .publish == null or (.publish | index("crates-io"));

  [.packages[]
    | select(.source == null)
    | {
        name,
        version,
        publish,
        deps: [.dependencies[]
          | select(.path != null and (.kind != "dev"))
          | {name, optional}
        ]
      }
  ] as $packages
  | ($packages | map({key: .name, value: .}) | from_entries) as $by_name
  | $packages[]
  | select(crates_io_publishable) as $package
  | $package.deps[]?
  | . as $dependency
  | ($by_name[$dependency.name]) as $dependency_package
  | select($dependency_package != null and ($dependency_package | crates_io_publishable | not))
  | [
      $package.name,
      $package.version,
      $dependency.name,
      $dependency_package.version,
      ($dependency.optional | tostring)
    ]
  | @tsv
' "$metadata_file" > "$blocked_dependencies_file"

curl_crate_version() {
  local crate_name=$1
  local crate_version=$2

  : > "$response_file"
  curl --proto '=https' --tlsv1.2 --silent --show-error --location \
    --retry "$curl_retries" \
    --retry-all-errors \
    --connect-timeout "$curl_connect_timeout" \
    --max-time "$curl_max_time" \
    --header "User-Agent: ${cargo_publish_user_agent}" \
    --output "$response_file" \
    --write-out '%{http_code}' \
    "${cargo_registry_api_url}/crates/${crate_name}/${crate_version}"
}

sparse_index_path() {
  local crate_name=${1,,}
  local crate_name_length=${#crate_name}

  case "$crate_name_length" in
    1)
      printf '1/%s' "$crate_name"
      ;;
    2)
      printf '2/%s' "$crate_name"
      ;;
    3)
      printf '3/%s/%s' "${crate_name:0:1}" "$crate_name"
      ;;
    *)
      printf '%s/%s/%s' "${crate_name:0:2}" "${crate_name:2:2}" "$crate_name"
      ;;
  esac
}

curl_sparse_index() {
  local crate_name=$1
  local index_path

  index_path="$(sparse_index_path "$crate_name")"

  : > "$index_response_file"
  curl --proto '=https' --tlsv1.2 --silent --show-error --location \
    --retry "$curl_retries" \
    --retry-all-errors \
    --connect-timeout "$curl_connect_timeout" \
    --max-time "$curl_max_time" \
    --header "User-Agent: ${cargo_publish_user_agent}" \
    --output "$index_response_file" \
    --write-out '%{http_code}' \
    "${cargo_sparse_index_url%/}/${index_path}"
}

version_exists() {
  local crate_name=$1
  local crate_version=$2
  local http_status

  http_status="$(curl_crate_version "$crate_name" "$crate_version")"
  case "$http_status" in
    200)
      return 0
      ;;
    404)
      return 1
      ;;
    *)
      echo "::error::crates.io lookup failed for ${crate_name} ${crate_version} (HTTP ${http_status})."
      cat "$response_file"
      echo
      return 2
      ;;
  esac
}

index_version_exists() {
  local crate_name=$1
  local crate_version=$2
  local http_status

  http_status="$(curl_sparse_index "$crate_name")"
  case "$http_status" in
    200)
      jq -e --arg version "$crate_version" 'select(.vers == $version)' \
        "$index_response_file" > /dev/null
      ;;
    404)
      return 1
      ;;
    *)
      echo "::error::crates.io sparse index lookup failed for ${crate_name} (HTTP ${http_status})."
      cat "$index_response_file"
      echo
      return 2
      ;;
  esac
}

check_blocked_dependencies() {
  local failed=false
  local exists_status=0
  local package_name
  local package_version
  local dependency_name
  local dependency_version
  local optional

  while IFS=$'\t' read -r package_name package_version dependency_name dependency_version optional; do
    if [[ -z "$package_name" ]]; then
      continue
    fi

    set +e
    version_exists "$dependency_name" "$dependency_version"
    exists_status=$?
    set -e

    if [[ "$exists_status" -eq 0 ]]; then
      continue
    fi
    if [[ "$exists_status" -ne 1 ]]; then
      exit "$exists_status"
    fi

    echo "::error::Cannot publish ${package_name} ${package_version}: local dependency"
    echo "::error::${dependency_name} ${dependency_version} is marked publish=false and is absent from crates.io."
    echo "::error::optional=${optional}. Publish the dependency first or mark ${package_name} publish=false."
    failed=true
  done < "$blocked_dependencies_file"

  if [[ "$failed" == true ]]; then
    exit 1
  fi
}

wait_for_version() {
  local crate_name=$1
  local crate_version=$2
  local api_ready=false
  local api_deadline
  local exists_status=0
  local index_deadline=0
  local index_ready=false

  api_deadline=$((SECONDS + cargo_publish_wait_timeout_seconds))

  while true; do
    if [[ "$api_ready" == false ]]; then
      set +e
      version_exists "$crate_name" "$crate_version"
      exists_status=$?
      set -e

      if [[ "$exists_status" -eq 0 ]]; then
        api_ready=true
        index_deadline=$((SECONDS + cargo_publish_wait_timeout_seconds))
      elif [[ "$exists_status" -ne 1 ]]; then
        return "$exists_status"
      fi
    fi

    if [[ "$api_ready" == true && "$index_ready" == false ]]; then
      set +e
      index_version_exists "$crate_name" "$crate_version"
      exists_status=$?
      set -e

      if [[ "$exists_status" -eq 0 ]]; then
        index_ready=true
      elif [[ "$exists_status" -ne 1 ]]; then
        return "$exists_status"
      fi
    fi

    if [[ "$api_ready" == true && "$index_ready" == true ]]; then
      return 0
    fi

    if [[ "$api_ready" == false && "$SECONDS" -ge "$api_deadline" ]]; then
      echo "::error::Timed out waiting for ${crate_name} ${crate_version} in the crates.io API."
      return 1
    fi

    if [[ "$api_ready" == true && "$SECONDS" -ge "$index_deadline" ]]; then
      echo "::error::Timed out waiting for ${crate_name} ${crate_version} in the crates.io sparse index."
      return 1
    fi

    sleep "$cargo_publish_poll_seconds"
  done
}

retry_sleep() {
  local attempt=$1
  local sleep_seconds=$((2 ** attempt))

  if [[ "$sleep_seconds" -lt "$cargo_publish_retry_delay_seconds" ]]; then
    sleep_seconds="$cargo_publish_retry_delay_seconds"
  fi

  sleep "$sleep_seconds"
}

success_sleep() {
  if [[ "$cargo_publish_success_delay_seconds" -gt 0 ]]; then
    sleep "$cargo_publish_success_delay_seconds"
  fi
}

publish_crate() {
  local crate_name=$1
  local crate_version=$2
  local exists_status=0
  local status=0

  set +e
  version_exists "$crate_name" "$crate_version"
  exists_status=$?
  set -e

  if [[ "$exists_status" -eq 0 ]]; then
    echo "Skipping ${crate_name} ${crate_version}: already published."
    return 0
  fi
  if [[ "$exists_status" -ne 1 ]]; then
    return "$exists_status"
  fi

  for attempt in $(seq 1 "$cargo_publish_attempts"); do
    echo "Publishing ${crate_name} ${crate_version} (attempt ${attempt}/${cargo_publish_attempts})"

    set +e
    cargo publish --locked --package "$crate_name"
    status=$?
    set -e

    if [[ "$status" -eq 0 ]]; then
      wait_for_version "$crate_name" "$crate_version"
      success_sleep
      return 0
    fi

    set +e
    version_exists "$crate_name" "$crate_version"
    exists_status=$?
    set -e

    if [[ "$exists_status" -eq 0 ]]; then
      echo "Treating ${crate_name} ${crate_version} as published after cargo exited ${status}."
      wait_for_version "$crate_name" "$crate_version"
      success_sleep
      return 0
    fi
    if [[ "$exists_status" -ne 1 ]]; then
      return "$exists_status"
    fi

    if [[ "$attempt" -lt "$cargo_publish_attempts" ]]; then
      echo "cargo publish failed for ${crate_name} (exit=${status}), retrying (${attempt}/${cargo_publish_attempts})"
      retry_sleep "$attempt"
    fi
  done

  echo "::error::Failed to publish ${crate_name} ${crate_version} after ${cargo_publish_attempts} attempts."
  return "$status"
}

if [[ ! -s "$publish_plan_file" ]]; then
  echo "::error::No publishable workspace crates found."
  exit 1
fi

check_blocked_dependencies

echo "Publishing crates in dependency order:"
nl -w1 -s'. ' "$publish_plan_file"

if [[ "$check_only" == true ]]; then
  echo "Cargo crate publish plan is valid."
  exit 0
fi

while IFS=$'\t' read -r crate_name crate_version; do
  publish_crate "$crate_name" "$crate_version"
done < "$publish_plan_file"

echo "Finished publishing Cargo crates."
