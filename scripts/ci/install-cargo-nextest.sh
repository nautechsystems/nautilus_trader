#!/usr/bin/env bash
set -euo pipefail

version="${1:-}"
attempts="${2:-3}"

if [ -z "$version" ]; then
  echo "Usage: $0 <version> [attempts]" >&2
  exit 1
fi

if ! [[ "$attempts" =~ ^[0-9]+$ ]] || [ "$attempts" -lt 1 ]; then
  echo "Attempt count must be a positive integer" >&2
  exit 1
fi

release_tag="cargo-nextest-${version}"
archive_name="${release_tag}-universal-apple-darwin.tar.gz"
checksum_name="${release_tag}-universal-apple-darwin.sha256"
base_url="https://github.com/nextest-rs/nextest/releases/download/${release_tag}"
bin_dir="${CARGO_HOME:-$HOME/.cargo}/bin"

if [ -x "${bin_dir}/cargo-nextest" ]; then
  installed_version="$("${bin_dir}/cargo-nextest" --version | sed -n '1{s/^cargo-nextest //; s/ .*//; p;}')"
  if [ "$installed_version" = "$version" ]; then
    echo "cargo-nextest ${version} is already installed"
    exit 0
  fi
fi

mkdir -p "$bin_dir"

if [ -n "${GITHUB_PATH:-}" ]; then
  echo "$bin_dir" >> "$GITHUB_PATH"
fi

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

for attempt in $(seq 1 "$attempts"); do
  archive_path="${work_dir}/${archive_name}"
  checksum_path="${work_dir}/${checksum_name}"

  rm -f "$archive_path" "$checksum_path" "${work_dir}/cargo-nextest"

  echo "Installing cargo-nextest ${version} (attempt ${attempt}/${attempts})"

  if ! curl -fsSL --retry 3 --retry-all-errors --connect-timeout 20 \
    -o "$archive_path" "${base_url}/${archive_name}"; then
    echo "Failed to download ${archive_name}"
  elif ! curl -fsSL --retry 3 --retry-all-errors --connect-timeout 20 \
    -o "$checksum_path" "${base_url}/${checksum_name}"; then
    echo "Failed to download ${checksum_name}"
  elif ! (
    cd "$work_dir"
    shasum -a 256 -c "$checksum_name"
  ); then
    echo "Checksum verification failed for ${archive_name}"
  elif ! tar -xzf "$archive_path" -C "$work_dir"; then
    echo "Failed to extract ${archive_name}"
  elif ! install -m 0755 "${work_dir}/cargo-nextest" "${bin_dir}/cargo-nextest"; then
    echo "Failed to install cargo-nextest into ${bin_dir}"
  else
    installed_version="$("${bin_dir}/cargo-nextest" --version | sed -n '1{s/^cargo-nextest //; s/ .*//; p;}')"
    if [ "$installed_version" = "$version" ]; then
      echo "Installed cargo-nextest ${version}"
      exit 0
    fi

    echo "Installed cargo-nextest but found version '${installed_version}'"
    rm -f "${bin_dir}/cargo-nextest"
  fi

  if [ "$attempt" -lt "$attempts" ]; then
    sleep 5
  fi
done

echo "::error::Failed to install cargo-nextest ${version} after ${attempts} attempts"
exit 1
