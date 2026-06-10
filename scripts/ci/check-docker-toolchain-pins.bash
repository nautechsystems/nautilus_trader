#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
UV_VERSION="$(bash "${REPO_ROOT}/scripts/uv-version.sh")"
RUST_VERSION="$(bash "${REPO_ROOT}/scripts/rust-toolchain.sh")"
EXPECTED_UV_PREFIX="ghcr.io/astral-sh/uv:${UV_VERSION}@sha256:"
EXPECTED_RUST_PREFIX="rust:${RUST_VERSION}-slim-bookworm@sha256:"

status=0

check_refs() {
  local label="$1"
  local expected_prefix="$2"
  local pattern="$3"
  shift 3

  local found=0
  local file
  local ref
  local digest

  for file in "$@"; do
    while IFS= read -r ref; do
      found=1
      digest="${ref##*@sha256:}"

      if [[ "$ref" != "$expected_prefix"* ]]; then
        echo "Error: ${file} uses ${ref}, expected ${label} from project config" >&2
        status=1
      fi

      if [[ ! "$digest" =~ ^[0-9a-f]{64}$ ]]; then
        echo "Error: ${file} uses an invalid ${label} digest: ${ref}" >&2
        status=1
      fi
    done < <(grep -Eo "$pattern" "${REPO_ROOT}/${file}" || true)
  done

  if [[ "$found" -eq 0 ]]; then
    echo "Error: no digest-pinned ${label} references found in Dockerfiles" >&2
    status=1
  fi
}

check_refs \
  "uv ${UV_VERSION}" \
  "$EXPECTED_UV_PREFIX" \
  'ghcr\.io/astral-sh/uv:[^[:space:]\\]+@sha256:[0-9a-f]+' \
  ".docker/DockerfileUbuntu" \
  ".docker/nautilus_trader.dockerfile" \
  ".docker/jupyterlab.dockerfile"

check_refs \
  "Rust ${RUST_VERSION}" \
  "$EXPECTED_RUST_PREFIX" \
  'rust:[^[:space:]\\]+@sha256:[0-9a-f]+' \
  ".docker/DockerfileUbuntu" \
  ".docker/nautilus_trader.dockerfile"

exit "$status"
