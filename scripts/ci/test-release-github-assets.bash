#!/usr/bin/env bash
# Test GitHub release asset checks and checksum publication with mocked gh calls.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/../.." && pwd)"
work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

fail() {
  echo "::error::$1" >&2
  exit 1
}

assert_file_exists() {
  local path=$1
  local message=$2

  if [[ ! -f "$path" ]]; then
    fail "$message"
  fi
}

assert_contains() {
  local path=$1
  local pattern=$2
  local message=$3

  if ! grep -Fq -- "$pattern" "$path"; then
    echo "expected pattern: $pattern" >&2
    echo "actual output:" >&2
    cat "$path" >&2
    fail "$message"
  fi
}

assert_not_contains() {
  local path=$1
  local pattern=$2
  local message=$3

  if grep -Fq -- "$pattern" "$path"; then
    echo "unexpected pattern: $pattern" >&2
    echo "actual output:" >&2
    cat "$path" >&2
    fail "$message"
  fi
}

write_portability_bash_env() {
  local output=$1

  cat > "$output" << 'BASHENV'
command() {
  if [[ "${1:-}" == "-v" && "${2:-}" == "sha256sum" ]]; then
    printf '%s\n' "hide-sha256sum" >> "${MOCK_PORTABILITY_LOG:?}"
    return 1
  fi

  builtin command "$@"
}

stat() {
  if [[ "${1:-}" == "-c" ]]; then
    printf '%s\n' "hide-stat-c" >> "${MOCK_PORTABILITY_LOG:?}"
    return 1
  fi

  if [[ "${1:-}" == "-f" && "${2:-}" == "%z" ]]; then
    printf '%s\n' "use-stat-f" >> "${MOCK_PORTABILITY_LOG:?}"
    wc -c < "$3" | tr -d ' '
    printf '\n'
    return 0
  fi

  /usr/bin/stat "$@"
}
BASHENV
}

write_mock_commands() {
  local mock_bin=$1

  mkdir -p "$mock_bin"

  cat > "${mock_bin}/gh" << 'MOCK'
#!/usr/bin/env bash
set -euo pipefail

copy_release_asset() {
  local name=$1
  local source_dir=${MOCK_GH_RELEASE_SOURCE:?}
  local output_dir=${MOCK_GH_DOWNLOAD_DIR:?}

  if [[ ! -f "${source_dir}/${name}" ]]; then
    return 0
  fi
  if [[ -e "${output_dir}/${name}" && "${MOCK_GH_DOWNLOAD_CLOBBER:-false}" != true ]]; then
    echo "asset already exists: ${name}" >&2
    exit 1
  fi

  cp "${source_dir}/${name}" "${output_dir}/${name}"
}

copy_release_suffix() {
  local suffix=$1
  local source_dir=${MOCK_GH_RELEASE_SOURCE:?}
  local path name

  for path in "${source_dir}"/*"${suffix}"; do
    [[ -e "$path" ]] || continue
    name=${path##*/}
    copy_release_asset "$name"
  done
}

printf '%s\n' "$*" >> "${MOCK_GH_LOG:?}"

if [[ "${1:-}" != "release" ]]; then
  echo "unexpected gh command: $*" >&2
  exit 2
fi

subcommand=${2:-}
shift 2

case "$subcommand" in
  view)
    json_fields=""
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --json)
          json_fields=$2
          shift 2
          ;;
        --jq | --repo)
          shift 2
          ;;
        *)
          shift
          ;;
      esac
    done

    case "$json_fields" in
      isDraft,assets)
        cat "${MOCK_GH_RELEASE_JSON:?}"
        ;;
      body)
        cat "${MOCK_GH_RELEASE_BODY:?}"
        ;;
      *)
        echo "unexpected gh release view fields: ${json_fields}" >&2
        exit 2
        ;;
    esac
    ;;
  download)
    patterns=()
    MOCK_GH_DOWNLOAD_CLOBBER=false
    MOCK_GH_DOWNLOAD_DIR=""

    while [[ $# -gt 0 ]]; do
      case "$1" in
        --dir)
          MOCK_GH_DOWNLOAD_DIR=$2
          shift 2
          ;;
        --pattern)
          patterns+=("$2")
          shift 2
          ;;
        --repo)
          shift 2
          ;;
        --clobber)
          MOCK_GH_DOWNLOAD_CLOBBER=true
          shift
          ;;
        *)
          shift
          ;;
      esac
    done

    if [[ -z "$MOCK_GH_DOWNLOAD_DIR" ]]; then
      echo "gh release download mock missing --dir" >&2
      exit 2
    fi
    mkdir -p "$MOCK_GH_DOWNLOAD_DIR"

    if [[ "${MOCK_GH_DOWNLOAD_ALWAYS_FAIL:-false}" == true ]]; then
      echo "mock persistent download failure" >&2
      exit 7
    fi

    if [[ "${MOCK_GH_DOWNLOAD_FAIL_ONCE:-false}" == true ]]; then
      count_file=${MOCK_GH_DOWNLOAD_COUNT_FILE:?}
      count=0
      if [[ -f "$count_file" ]]; then
        count="$(cat "$count_file")"
      fi
      if [[ "$count" == "0" ]]; then
        printf '1\n' > "$count_file"
        copy_release_asset dist-manifest.json
        echo "mock transient download failure" >&2
        exit 7
      fi
    fi

    for pattern in "${patterns[@]}"; do
      case "$pattern" in
        "*.whl")
          copy_release_suffix ".whl"
          ;;
        "*.tar.gz")
          copy_release_suffix ".tar.gz"
          ;;
        dist-manifest.json | crates-manifest.json)
          copy_release_asset "$pattern"
          ;;
        *)
          echo "unexpected gh release download pattern: ${pattern}" >&2
          exit 2
          ;;
      esac
    done
    ;;
  upload)
    tag_seen=false
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --repo)
          shift 2
          ;;
        --clobber)
          shift
          ;;
        --*)
          echo "unexpected gh release upload option: $1" >&2
          exit 2
          ;;
        *)
          if [[ "$tag_seen" == false ]]; then
            tag_seen=true
          else
            printf '%s\n' "${1##*/}" >> "${MOCK_GH_UPLOAD_LOG:?}"
          fi
          shift
          ;;
      esac
    done
    ;;
  edit)
    notes_file=""
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --notes-file)
          notes_file=$2
          shift 2
          ;;
        --repo)
          shift 2
          ;;
        *)
          shift
          ;;
      esac
    done

    if [[ -z "$notes_file" ]]; then
      echo "gh release edit mock missing --notes-file" >&2
      exit 2
    fi
    cp "$notes_file" "${MOCK_GH_EDIT_BODY:?}"
    ;;
  *)
    echo "unexpected gh release subcommand: ${subcommand}" >&2
    exit 2
    ;;
esac
MOCK

  cat > "${mock_bin}/sleep" << 'MOCK'
#!/usr/bin/env bash
set -euo pipefail
:
MOCK

  chmod +x "${mock_bin}/gh" "${mock_bin}/sleep"
}

write_release_source() {
  local release_source=$1

  mkdir -p "$release_source"
  printf 'wheel artifact\n' > "${release_source}/nautilus_trader-1.2.3-py3-none-any.whl"
  printf 'sdist artifact\n' > "${release_source}/nautilus_trader-1.2.3.tar.gz"
  cat > "${release_source}/dist-manifest.json" << 'JSON'
{
  "schema_version": 1,
  "tag": "v1.2.3",
  "artifacts": [
    {
      "name": "nautilus_trader-1.2.3-py3-none-any.whl",
      "sha256": "wheel-sha256",
      "size": 15
    }
  ]
}
JSON
  cat > "${release_source}/crates-manifest.json" << 'JSON'
{
  "schema_version": 1,
  "crates": [
    {
      "name": "nautilus-core",
      "version": "1.2.3"
    }
  ]
}
JSON
}

write_success_release_json() {
  local output=$1

  cat > "$output" << 'JSON'
{
  "isDraft": true,
  "assets": [
    {"name": "SHA256SUMS", "state": "uploaded", "size": 64},
    {"name": "dist-manifest.json", "state": "uploaded", "size": 120},
    {"name": "crates-manifest.json", "state": "uploaded", "size": 90},
    {"name": "nautilus_trader-1.2.3-py3-none-any.whl", "state": "uploaded", "size": 15},
    {"name": "nautilus_trader-1.2.3-py3-none-any.whl.sha256", "state": "uploaded", "size": 96},
    {"name": "nautilus_trader-1.2.3-py3-none-any.whl.sigstore", "state": "uploaded", "size": 96},
    {"name": "nautilus_trader-1.2.3-py3-none-any.whl.intoto.jsonl", "state": "uploaded", "size": 96}
  ]
}
JSON
}

write_release_json_with_draft_state() {
  local output=$1

  cat > "$output" << 'JSON'
{
  "isDraft": false,
  "assets": []
}
JSON
}

write_release_json_with_duplicates() {
  local output=$1

  cat > "$output" << 'JSON'
{
  "isDraft": true,
  "assets": [
    {"name": "SHA256SUMS", "state": "uploaded", "size": 64},
    {"name": "SHA256SUMS", "state": "uploaded", "size": 64}
  ]
}
JSON
}

write_release_json_with_incomplete_asset() {
  local output=$1

  cat > "$output" << 'JSON'
{
  "isDraft": true,
  "assets": [
    {"name": "SHA256SUMS", "state": "uploaded", "size": 64},
    {"name": "dist-manifest.json", "state": "uploaded", "size": 120},
    {"name": "crates-manifest.json", "state": "uploaded", "size": 0}
  ]
}
JSON
}

write_release_json_missing_sigstore() {
  local output=$1

  cat > "$output" << 'JSON'
{
  "isDraft": true,
  "assets": [
    {"name": "SHA256SUMS", "state": "uploaded", "size": 64},
    {"name": "dist-manifest.json", "state": "uploaded", "size": 120},
    {"name": "crates-manifest.json", "state": "uploaded", "size": 90},
    {"name": "nautilus_trader-1.2.3-py3-none-any.whl", "state": "uploaded", "size": 15},
    {"name": "nautilus_trader-1.2.3-py3-none-any.whl.sha256", "state": "uploaded", "size": 96},
    {"name": "nautilus_trader-1.2.3-py3-none-any.whl.intoto.jsonl", "state": "uploaded", "size": 96}
  ]
}
JSON
}

run_verify_assets() {
  local release_json=$1
  local output=$2
  local fail_once=${3:-false}
  local always_fail=${4:-false}

  (
    cd "$repo_root"
    env \
      PATH="${mock_bin}:${PATH}" \
      GH_RELEASE_ASSET_VERIFY_ATTEMPTS=2 \
      GH_RELEASE_EXPECT_DRAFT=true \
      GITHUB_REPOSITORY=nautechsystems/nautilus_trader \
      GITHUB_TOKEN=mock-token \
      MOCK_GH_DOWNLOAD_ALWAYS_FAIL="$always_fail" \
      MOCK_GH_DOWNLOAD_COUNT_FILE="${work_dir}/download-count" \
      MOCK_GH_DOWNLOAD_FAIL_ONCE="$fail_once" \
      MOCK_GH_EDIT_BODY="${work_dir}/edited-notes.md" \
      MOCK_GH_LOG="${work_dir}/gh.log" \
      MOCK_GH_RELEASE_BODY="${work_dir}/release-body.md" \
      MOCK_GH_RELEASE_JSON="$release_json" \
      MOCK_GH_RELEASE_SOURCE="$release_source" \
      MOCK_GH_UPLOAD_LOG="${work_dir}/upload.log" \
      TAG_NAME=v1.2.3 \
      bash scripts/ci/verify-github-release-assets.bash
  ) > "$output" 2>&1
}

run_verify_failure() {
  local release_json=$1
  local output=$2
  local pattern=$3

  set +e
  run_verify_assets "$release_json" "$output"
  local status=$?
  set -e

  if [[ "$status" -eq 0 ]]; then
    cat "$output" >&2
    fail "release asset verification should fail."
  fi
  assert_contains "$output" "$pattern" "release asset verification failed for the wrong reason."
}

run_checksum_script() {
  local output=$1
  shift

  local bash_env="${CHECKSUM_BASH_ENV:-}"

  (
    cd "$repo_root"
    env \
      BASH_ENV="$bash_env" \
      PATH="${mock_bin}:${PATH}" \
      GITHUB_REPOSITORY=nautechsystems/nautilus_trader \
      GITHUB_TOKEN=mock-token \
      MOCK_GH_EDIT_BODY="${work_dir}/edited-notes.md" \
      MOCK_GH_LOG="${work_dir}/gh.log" \
      MOCK_PORTABILITY_LOG="${work_dir}/portability.log" \
      MOCK_GH_RELEASE_BODY="${work_dir}/release-body.md" \
      MOCK_GH_RELEASE_JSON="${work_dir}/success-release.json" \
      MOCK_GH_RELEASE_SOURCE="$release_source" \
      MOCK_GH_UPLOAD_LOG="${work_dir}/upload.log" \
      TAG_NAME=v1.2.3 \
      bash scripts/ci/publish-release-checksums.sh "$@"
  ) > "$output" 2>&1
}

mock_bin="${work_dir}/mock-bin"
release_source="${work_dir}/release-source"
write_mock_commands "$mock_bin"
write_release_source "$release_source"
printf 'Existing notes\n<!-- release-checksums:start -->\nold\n<!-- release-checksums:end -->\n' \
  > "${work_dir}/release-body.md"

success_json="${work_dir}/success-release.json"
write_success_release_json "$success_json"
verify_output="${work_dir}/verify-success.out"
run_verify_assets "$success_json" "$verify_output"
assert_contains "$verify_output" "Verified final release assets" \
  "complete release asset set should verify."

rm -f "${work_dir}/download-count"
retry_output="${work_dir}/verify-retry.out"
run_verify_assets "$success_json" "$retry_output" true
assert_contains "$retry_output" "Verified final release assets" \
  "manifest download retry should recover after a partial first attempt."
assert_contains "${work_dir}/gh.log" "--clobber" \
  "manifest download retry should use --clobber."

persistent_failure_output="${work_dir}/verify-persistent-download-failure.out"
set +e
run_verify_assets "$success_json" "$persistent_failure_output" false true
persistent_failure_status=$?
set -e
if [[ "$persistent_failure_status" -eq 0 ]]; then
  cat "$persistent_failure_output" >&2
  fail "persistent manifest download failures should fail verification."
fi
assert_contains "$persistent_failure_output" "gh release download manifests failed after retries." \
  "persistent manifest download failure should report retry exhaustion."

draft_json="${work_dir}/draft-mismatch.json"
write_release_json_with_draft_state "$draft_json"
run_verify_failure "$draft_json" "${work_dir}/draft-mismatch.out" \
  "draft state was false, expected true"

duplicate_json="${work_dir}/duplicate-assets.json"
write_release_json_with_duplicates "$duplicate_json"
run_verify_failure "$duplicate_json" "${work_dir}/duplicate-assets.out" \
  "contains duplicate asset names"

incomplete_json="${work_dir}/incomplete-assets.json"
write_release_json_with_incomplete_asset "$incomplete_json"
run_verify_failure "$incomplete_json" "${work_dir}/incomplete-assets.out" \
  "contains incomplete assets"

missing_sigstore_json="${work_dir}/missing-sigstore.json"
write_release_json_missing_sigstore "$missing_sigstore_json"
run_verify_failure "$missing_sigstore_json" "${work_dir}/missing-sigstore.out" \
  "missing asset: nautilus_trader-1.2.3-py3-none-any.whl.sigstore"

generated_assets="${work_dir}/generated-assets"
: > "${work_dir}/gh.log"
: > "${work_dir}/upload.log"
run_checksum_script "${work_dir}/generate-only.out" --generate-only "$generated_assets"
assert_file_exists "${generated_assets}/SHA256SUMS" "generate-only should write SHA256SUMS."
assert_file_exists "${generated_assets}/dist-manifest.json" \
  "generate-only should write dist-manifest.json."
assert_file_exists "${generated_assets}/nautilus_trader-1.2.3-py3-none-any.whl.sha256" \
  "generate-only should write the wheel checksum."
assert_file_exists "${generated_assets}/nautilus_trader-1.2.3.tar.gz.sha256" \
  "generate-only should write the sdist checksum."
if ! jq -e '.artifacts | length == 2' "${generated_assets}/dist-manifest.json" > /dev/null; then
  fail "generate-only should record both Python release artifacts."
fi
assert_not_contains "${work_dir}/gh.log" "release upload" \
  "generate-only should not upload release assets."

portability_bash_env="${work_dir}/portability.bashenv"
portable_assets="${work_dir}/portable-assets"
write_portability_bash_env "$portability_bash_env"
: > "${work_dir}/portability.log"
CHECKSUM_BASH_ENV="$portability_bash_env" \
  run_checksum_script "${work_dir}/portable-generate-only.out" --generate-only "$portable_assets"
assert_file_exists "${portable_assets}/SHA256SUMS" \
  "portable generate-only should write SHA256SUMS."
assert_file_exists "${portable_assets}/dist-manifest.json" \
  "portable generate-only should write dist-manifest.json."
assert_contains "${work_dir}/portability.log" "hide-sha256sum" \
  "portable generate-only should exercise the shasum fallback."
assert_contains "${work_dir}/portability.log" "hide-stat-c" \
  "portable generate-only should exercise the stat -f fallback."
assert_contains "${work_dir}/portability.log" "use-stat-f" \
  "portable generate-only should use the BSD stat size branch."

: > "${work_dir}/gh.log"
: > "${work_dir}/upload.log"
run_checksum_script "${work_dir}/publish-existing.out" --publish-existing "$generated_assets"
assert_contains "${work_dir}/upload.log" "SHA256SUMS" \
  "publish-existing should upload SHA256SUMS."
assert_contains "${work_dir}/upload.log" "dist-manifest.json" \
  "publish-existing should upload dist-manifest.json."
assert_contains "${work_dir}/upload.log" "nautilus_trader-1.2.3-py3-none-any.whl.sha256" \
  "publish-existing should upload per-wheel checksums."
assert_contains "${work_dir}/upload.log" "nautilus_trader-1.2.3.tar.gz.sha256" \
  "publish-existing should upload per-sdist checksums."
assert_contains "${work_dir}/edited-notes.md" "## Artifact checksums" \
  "publish-existing should add the checksum release notes block."
assert_not_contains "${work_dir}/edited-notes.md" "old" \
  "publish-existing should replace an existing checksum release notes block."

missing_checksum_assets="${work_dir}/missing-checksum-assets"
mkdir -p "$missing_checksum_assets"
printf '0123456789abcdef  missing.whl\n' > "${missing_checksum_assets}/SHA256SUMS"
jq -n '{artifacts: [{name: "missing.whl"}]}' > "${missing_checksum_assets}/dist-manifest.json"
: > "${work_dir}/upload.log"
set +e
run_checksum_script "${work_dir}/publish-missing-checksum.out" \
  --publish-existing "$missing_checksum_assets"
missing_checksum_status=$?
set -e
if [[ "$missing_checksum_status" -eq 0 ]]; then
  fail "publish-existing should fail before upload when per-asset checksums are missing."
fi
assert_contains "${work_dir}/publish-missing-checksum.out" "Missing per-asset checksum files" \
  "publish-existing missing-checksum failure should name the missing checksum."
if [[ -s "${work_dir}/upload.log" ]]; then
  cat "${work_dir}/upload.log" >&2
  fail "publish-existing should not upload when checksum validation fails."
fi

echo "release GitHub asset tests passed."
