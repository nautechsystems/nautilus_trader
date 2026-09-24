#!/usr/bin/env bash
set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
create_script="$repo_root/scripts/ci/create-docker-manifest.bash"
save_script="$repo_root/scripts/ci/save-docker-digest.bash"
case_root=$(mktemp -d)
trap 'rm -rf "$case_root"' EXIT
fake_bin="$case_root/bin"
mkdir -p "$fake_bin"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  'set -euo pipefail' \
  "printf '%s\\n' \"\$*\" >> \"\$DOCKER_LOG\"" \
  'if [[ "$*" == *"imagetools inspect"* ]]; then' \
  '  printf "{\"digest\":\"sha256:%064d\"}\n" 3' \
  'fi' \
  > "$fake_bin/docker"
chmod +x "$fake_bin/docker"

saved_digest="sha256:$(printf '%064d' 1)"
RUNNER_TEMP="$case_root/runner" bash "$save_script" nautilus_trader "$saved_digest"
test -f "$case_root/runner/digests/nautilus_trader/${saved_digest#sha256:}"

status=0
diagnostic=$(RUNNER_TEMP="$case_root/runner" bash "$save_script" '../invalid' "$saved_digest" 2>&1) || status=$?
expected='::error::Invalid image artifact name: ../invalid'
if [ "$status" -ne 1 ] || [ "$diagnostic" != "$expected" ]; then
  printf 'Expected status 1 and diagnostic: %s\n' "$expected" >&2
  printf 'Actual status %s and diagnostic: %s\n' "$status" "$diagnostic" >&2
  exit 1
fi
status=0
diagnostic=$(RUNNER_TEMP="$case_root/runner" bash "$save_script" jupyterlab invalid 2>&1) || status=$?
expected='::error::Invalid image digest: invalid'
if [ "$status" -ne 1 ] || [ "$diagnostic" != "$expected" ]; then
  printf 'Expected status 1 and diagnostic: %s\n' "$expected" >&2
  printf 'Actual status %s and diagnostic: %s\n' "$status" "$diagnostic" >&2
  exit 1
fi

manifest_dir="$case_root/manifest"
mkdir -p "$manifest_dir"
digest_one=$(printf '%064d' 1)
digest_two=$(printf '%064d' 2)
touch "$manifest_dir/$digest_one" "$manifest_dir/$digest_two"
metadata='{"tags":["ghcr.io/nautechsystems/nautilus_trader:nightly","ghcr.io/nautechsystems/nautilus_trader:latest"]}'
output="$case_root/github-output"
docker_log="$case_root/docker-log"
(
  cd "$manifest_dir"
  PATH="$fake_bin:$PATH" \
    DOCKER_LOG="$docker_log" \
    DOCKER_METADATA_OUTPUT_JSON="$metadata" \
    GITHUB_OUTPUT="$output" \
    bash "$create_script" ghcr.io/nautechsystems/nautilus_trader
)

grep -Fxq "digest=sha256:$(printf '%064d' 3)" "$output"
grep -Fxq "digest_hex=$(printf '%064d' 3)" "$output"
grep -Fq -- "-t ghcr.io/nautechsystems/nautilus_trader:nightly" "$docker_log"
grep -Fq "ghcr.io/nautechsystems/nautilus_trader@sha256:$digest_one" "$docker_log"
grep -Fq "ghcr.io/nautechsystems/nautilus_trader@sha256:$digest_two" "$docker_log"

invalid_dir="$case_root/invalid-digest"
mkdir -p "$invalid_dir"
touch "$invalid_dir/not-a-digest"
status=0
diagnostic=$(
  exec 2>&1
  cd "$invalid_dir"
  PATH="$fake_bin:$PATH" \
    DOCKER_LOG="$docker_log" \
    DOCKER_METADATA_OUTPUT_JSON="$metadata" \
    GITHUB_OUTPUT="$output" \
    bash "$create_script" ghcr.io/nautechsystems/nautilus_trader
) || status=$?
expected='::error::Invalid image digest file: not-a-digest'
if [ "$status" -ne 1 ] || [ "$diagnostic" != "$expected" ]; then
  printf 'Expected status 1 and diagnostic: %s\n' "$expected" >&2
  printf 'Actual status %s and diagnostic: %s\n' "$status" "$diagnostic" >&2
  exit 1
fi

mismatched_metadata='{"tags":["ghcr.io/other/image:nightly"]}'
status=0
diagnostic=$(
  exec 2>&1
  cd "$manifest_dir"
  PATH="$fake_bin:$PATH" \
    DOCKER_LOG="$docker_log" \
    DOCKER_METADATA_OUTPUT_JSON="$mismatched_metadata" \
    GITHUB_OUTPUT="$output" \
    bash "$create_script" ghcr.io/nautechsystems/nautilus_trader
) || status=$?
expected='::error::Invalid Docker metadata tag for ghcr.io/nautechsystems/nautilus_trader: ghcr.io/other/image:nightly'
if [ "$status" -ne 1 ] || [ "$diagnostic" != "$expected" ]; then
  printf 'Expected status 1 and diagnostic: %s\n' "$expected" >&2
  printf 'Actual status %s and diagnostic: %s\n' "$status" "$diagnostic" >&2
  exit 1
fi

malformed_metadata='{"tags":["ghcr.io/nautechsystems/nautilus_trader:bad tag"]}'
: > "$docker_log"
status=0
diagnostic=$(
  exec 2>&1
  cd "$manifest_dir"
  PATH="$fake_bin:$PATH" \
    DOCKER_LOG="$docker_log" \
    DOCKER_METADATA_OUTPUT_JSON="$malformed_metadata" \
    GITHUB_OUTPUT="$output" \
    bash "$create_script" ghcr.io/nautechsystems/nautilus_trader
) || status=$?
expected='::error::Invalid Docker metadata tag for ghcr.io/nautechsystems/nautilus_trader: ghcr.io/nautechsystems/nautilus_trader:bad tag'
if [ "$status" -ne 1 ] || [ "$diagnostic" != "$expected" ]; then
  printf 'Expected status 1 and diagnostic: %s\n' "$expected" >&2
  printf 'Actual status %s and diagnostic: %s\n' "$status" "$diagnostic" >&2
  exit 1
fi
if [ -s "$docker_log" ]; then
  echo "Expected malformed metadata to fail before invoking Docker" >&2
  exit 1
fi

empty_dir="$case_root/empty"
mkdir -p "$empty_dir"
status=0
diagnostic=$(
  exec 2>&1
  cd "$empty_dir"
  PATH="$fake_bin:$PATH" \
    DOCKER_LOG="$docker_log" \
    DOCKER_METADATA_OUTPUT_JSON="$metadata" \
    GITHUB_OUTPUT="$output" \
    bash "$create_script" ghcr.io/nautechsystems/nautilus_trader
) || status=$?
expected='::error::No image digest files found'
if [ "$status" -ne 1 ] || [ "$diagnostic" != "$expected" ]; then
  printf 'Expected status 1 and diagnostic: %s\n' "$expected" >&2
  printf 'Actual status %s and diagnostic: %s\n' "$status" "$diagnostic" >&2
  exit 1
fi

echo "Docker workflow script tests passed"
