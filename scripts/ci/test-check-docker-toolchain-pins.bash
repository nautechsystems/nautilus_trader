#!/usr/bin/env bash

set -euo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)
CHECKER="$REPO_ROOT/scripts/ci/check-docker-toolchain-pins.bash"

CASE_ROOT=$(mktemp -d)
trap 'rm -rf "$CASE_ROOT"' EXIT

DIGEST="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

create_case() {
  local name="$1"
  local uv_version="$2"
  local digest="$3"
  local python_version="$4"
  local site_version="$5"
  local install_version="$6"
  local case_dir="$CASE_ROOT/$name"

  mkdir -p "$case_dir/scripts/ci" "$case_dir/.docker" "$case_dir/python"
  cp "$CHECKER" "$case_dir/scripts/ci/check-docker-toolchain-pins.bash"
  cp \
    "$REPO_ROOT/scripts/rust-toolchain.sh" \
    "$REPO_ROOT/scripts/tool-version.sh" \
    "$REPO_ROOT/scripts/uv-version.sh" \
    "$case_dir/scripts/"

  printf '%s\n' '[uv]' 'version = "0.12.3"' > "$case_dir/tools.toml"
  printf '%s\n' '[toolchain]' 'channel = "1.98.0"' > "$case_dir/rust-toolchain.toml"
  printf '%s\n' '[project]' 'requires-python = ">=3.12,<3.15"' > "$case_dir/python/pyproject.toml"

  printf '%s\n' \
    "FROM public.ecr.aws/docker/library/ubuntu@sha256:$digest" \
    "COPY --from=ghcr.io/astral-sh/uv:$uv_version@sha256:$digest /uv /bin/uv" \
    "COPY --from=public.ecr.aws/docker/library/rust:1.98.0-slim-bookworm@sha256:$digest /usr/local/cargo /usr/local/cargo" \
    "RUN uv python install $install_version" > "$case_dir/.docker/DockerfileUbuntu"
  printf '%s\n' \
    "FROM public.ecr.aws/docker/library/python:$python_version-slim@sha256:$digest" \
    "COPY --from=ghcr.io/astral-sh/uv:$uv_version@sha256:$digest /uv /bin/uv" \
    "COPY --from=public.ecr.aws/docker/library/rust:1.98.0-slim-bookworm@sha256:$digest /usr/local/cargo /usr/local/cargo" \
    "ENV PYTHONPATH=/opt/venv/lib/python$site_version/site-packages" > "$case_dir/.docker/nautilus_trader.dockerfile"
  printf '%s\n' \
    "COPY --from=ghcr.io/astral-sh/uv:$uv_version@sha256:$digest /uv /bin/uv" \
    > "$case_dir/.docker/jupyterlab.dockerfile"

  printf '%s' "$case_dir"
}

run_checker() {
  local case_dir="$1"

  set +e
  bash "$case_dir/scripts/ci/check-docker-toolchain-pins.bash" \
    > "$case_dir/stdout.txt" 2> "$case_dir/stderr.txt"
  RUN_STATUS=$?
  set -e
}

expect_failure() {
  local case_dir="$1"
  local reason="$2"

  run_checker "$case_dir"
  if [ "$RUN_STATUS" -ne 1 ]; then
    echo "Expected Docker toolchain pin check to fail in $case_dir"
    cat "$case_dir/stdout.txt" "$case_dir/stderr.txt"
    exit 1
  fi
  if ! grep -Fq "$reason" "$case_dir/stderr.txt"; then
    echo "Expected Docker pin failure reason not found: $reason"
    cat "$case_dir/stderr.txt"
    exit 1
  fi
}

valid_case=$(create_case "valid" "0.12.3" "$DIGEST" "3.14" "3.14" "3.14")
run_checker "$valid_case"
if [ "$RUN_STATUS" -ne 0 ]; then
  echo "Expected Docker toolchain pin check to pass valid input"
  cat "$valid_case/stdout.txt" "$valid_case/stderr.txt"
  exit 1
fi

uv_case=$(create_case "uv-version" "0.11.0" "$DIGEST" "3.14" "3.14" "3.14")
expect_failure "$uv_case" "expected uv 0.12.3 from tools.toml [uv].version"

digest_case=$(create_case "invalid-digest" "0.12.3" "${DIGEST%?}" "3.14" "3.14" "3.14")
expect_failure "$digest_case" "uses an invalid uv 0.12.3 digest"

site_case=$(create_case "site-version" "0.12.3" "$DIGEST" "3.14" "3.13" "3.14")
expect_failure "$site_case" "references python3.13, expected python3.14 from the base image"

range_case=$(create_case "python-range" "0.12.3" "$DIGEST" "3.15" "3.15" "3.15")
expect_failure "$range_case" 'Python 3.15 is outside requires-python ">=3.12,<3.15"'

echo "Docker toolchain pin script tests passed"
