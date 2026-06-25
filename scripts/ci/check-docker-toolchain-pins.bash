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
  local version_source="$2"
  local expected_prefix="$3"
  local pattern="$4"
  shift 4

  local found=0
  local file
  local ref
  local digest

  for file in "$@"; do
    while IFS= read -r ref; do
      found=1
      digest="${ref##*@sha256:}"

      if [[ "$ref" != "$expected_prefix"* ]]; then
        echo "Error: ${file} uses ${ref}, expected ${label} from ${version_source}" >&2
        echo "       Update the Docker image tag and digest together." >&2
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

# The python base image has no canonical version file to align against, so its
# version tag is the source of truth: every other Python reference must match it,
# and it must stay within pyproject's requires-python range.
check_python_alignment() {
  local nautilus_df=".docker/nautilus_trader.dockerfile"
  local ubuntu_df=".docker/DockerfileUbuntu"
  local pyproject="pyproject.toml"
  local base_ref py_version minor digest requires lower upper ref found

  base_ref="$(grep -Eo 'python:3\.[0-9]+-slim@sha256:[0-9a-f]+' "${REPO_ROOT}/${nautilus_df}" | head -n1 || true)"
  if [[ -z "$base_ref" ]]; then
    echo "Error: ${nautilus_df} has no version-tagged, digest-pinned python base image" >&2
    echo "       Expected: FROM python:<major>.<minor>-slim@sha256:<digest>" >&2
    status=1
    return
  fi
  py_version="$(printf '%s' "$base_ref" | grep -Eo '3\.[0-9]+' | head -n1)"
  minor="${py_version#3.}"

  digest="${base_ref##*@sha256:}"
  if [[ ! "$digest" =~ ^[0-9a-f]{64}$ ]]; then
    echo "Error: ${nautilus_df} python base image has an invalid digest: ${base_ref}" >&2
    status=1
  fi

  requires="$(awk -F'"' '/^requires-python/{print $2; exit}' "${REPO_ROOT}/${pyproject}")"
  lower="$(printf '%s' "$requires" | grep -Eo '>=[[:space:]]*3\.[0-9]+' | grep -Eo '[0-9]+$' || true)"
  upper="$(printf '%s' "$requires" | grep -Eo '<[[:space:]]*3\.[0-9]+' | grep -Eo '[0-9]+$' || true)"
  if [[ -n "$lower" ]] && ((minor < lower)); then
    echo "Error: ${nautilus_df} Python ${py_version} is below requires-python \"${requires}\" in ${pyproject}" >&2
    status=1
  fi
  if [[ -n "$upper" ]] && ((minor >= upper)); then
    echo "Error: ${nautilus_df} Python ${py_version} is outside requires-python \"${requires}\" in ${pyproject}" >&2
    status=1
  fi

  found=0
  while IFS= read -r ref; do
    found=1
    if [[ "$ref" != "python${py_version}" ]]; then
      echo "Error: ${nautilus_df} references ${ref}, expected python${py_version} from the base image" >&2
      status=1
    fi
  done < <(grep -Eo 'python3\.[0-9]+' "${REPO_ROOT}/${nautilus_df}" || true)
  if [[ "$found" -eq 0 ]]; then
    echo "Error: ${nautilus_df} has no python<version> site-packages paths to verify" >&2
    status=1
  fi

  found=0
  while IFS= read -r ref; do
    found=1
    if [[ "$ref" != "$py_version" ]]; then
      echo "Error: ${ubuntu_df} installs Python ${ref}, expected ${py_version} from the base image" >&2
      status=1
    fi
  done < <(grep -Eo 'uv python install[[:space:]]+3\.[0-9]+' "${REPO_ROOT}/${ubuntu_df}" | grep -Eo '3\.[0-9]+' || true)
  if [[ "$found" -eq 0 ]]; then
    echo "Error: ${ubuntu_df} has no 'uv python install <version>' to verify" >&2
    status=1
  fi
}

check_refs \
  "uv ${UV_VERSION}" \
  "pyproject.toml [tool.uv].required-version" \
  "$EXPECTED_UV_PREFIX" \
  'ghcr\.io/astral-sh/uv:[^[:space:]\\]+@sha256:[0-9a-f]+' \
  ".docker/DockerfileUbuntu" \
  ".docker/nautilus_trader.dockerfile" \
  ".docker/jupyterlab.dockerfile"

check_refs \
  "Rust ${RUST_VERSION}" \
  "rust-toolchain.toml [toolchain].version" \
  "$EXPECTED_RUST_PREFIX" \
  'rust:[^[:space:]\\]+@sha256:[0-9a-f]+' \
  ".docker/DockerfileUbuntu" \
  ".docker/nautilus_trader.dockerfile"

check_python_alignment

exit "$status"
