#!/usr/bin/env bash
set -euo pipefail

version="${1:-}"
attempts="${2:-${INSTALL_ATTEMPTS:-5}}"
curl_retries="${CURL_RETRIES:-5}"
curl_connect_timeout="${CURL_CONNECT_TIMEOUT:-20}"
curl_max_time="${CURL_MAX_TIME:-300}"

if [ -z "$version" ]; then
  echo "Usage: $0 <version> [attempts]" >&2
  exit 1
fi

if ! [[ "$attempts" =~ ^[0-9]+$ ]] || [ "$attempts" -lt 1 ]; then
  echo "Attempt count must be a positive integer" >&2
  exit 1
fi

installer_url="https://github.com/j178/prek/releases/download/v${version}/prek-installer.sh"
bin_dirs=(
  "${PREK_BIN_DIR:-}"
  "${HOME}/.local/bin"
  "${CARGO_HOME:-$HOME/.cargo}/bin"
)

for bin_dir in "${bin_dirs[@]}"; do
  if [ -n "$bin_dir" ]; then
    PATH="${bin_dir}:$PATH"
  fi
done
export PATH

if [ -n "${GITHUB_PATH:-}" ]; then
  for bin_dir in "${bin_dirs[@]}"; do
    if [ -n "$bin_dir" ]; then
      echo "$bin_dir" >> "$GITHUB_PATH"
    fi
  done
fi

get_installed_version() {
  local version_output

  version_output="$(prek --version 2> /dev/null || true)"
  awk '
    NR == 1 {
      for (i = 1; i <= NF; i++) {
        if ($i ~ /^v?[0-9]+[.][0-9]+[.][0-9]+(-[0-9A-Za-z.-]+)?([+][0-9A-Za-z.-]+)?$/) {
          sub(/^v/, "", $i)
          print $i
          exit
        }
      }
    }
  ' <<< "$version_output"
}

is_requested_version() {
  case "$1" in
    "$version" | "$version"+*) return 0 ;;
    *) return 1 ;;
  esac
}

if command -v prek > /dev/null 2>&1; then
  installed_version="$(get_installed_version)"
  if is_requested_version "$installed_version"; then
    echo "prek ${version} is already installed"
    exit 0
  fi
fi

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

download_file() {
  local output_path="$1"
  local url="$2"

  curl --proto '=https' --tlsv1.2 -fsSL \
    --retry "$curl_retries" \
    --retry-all-errors \
    --connect-timeout "$curl_connect_timeout" \
    --max-time "$curl_max_time" \
    -o "$output_path" "$url"
}

for attempt in $(seq 1 "$attempts"); do
  installer_path="${work_dir}/prek-installer.sh"
  rm -f "$installer_path"

  echo "Installing prek ${version} (attempt ${attempt}/${attempts})"

  if ! download_file "$installer_path" "$installer_url"; then
    echo "Failed to download prek installer"
  elif ! sh "$installer_path"; then
    echo "Failed to run prek installer"
  elif ! command -v prek > /dev/null 2>&1; then
    echo "prek was not found on PATH after install"
  else
    installed_version="$(get_installed_version)"
    if is_requested_version "$installed_version"; then
      echo "Installed prek ${version}"
      exit 0
    fi

    echo "Installed prek but found version '${installed_version}'"
  fi

  if [ "$attempt" -lt "$attempts" ]; then
    sleep $((2 ** attempt))
  fi
done

echo "::error::Failed to install prek ${version} after ${attempts} attempts"
exit 1
