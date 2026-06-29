#!/usr/bin/env bash
# Test crates.io manual publish exceptions in the release registry verifier.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/../.." && pwd)"
work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

fail() {
  echo "::error::$1" >&2
  exit 1
}

sha256_file() {
  sha256sum "$1" | awk '{print $1}'
}

mock_bin="${work_dir}/mock-bin"
mkdir -p "$mock_bin"
export CURL_ATTEMPT_DIR="${work_dir}/curl-attempts"
mkdir -p "$CURL_ATTEMPT_DIR"

python_artifact="${work_dir}/nautilus_trader-1.2.3.tar.gz"
crate_artifact="${work_dir}/nautilus-core-1.2.3.crate"
printf 'python artifact\n' > "$python_artifact"
printf 'crate artifact\n' > "$crate_artifact"

export PYTHON_ARTIFACT_FILE="$python_artifact"
export CRATE_ARTIFACT_FILE="$crate_artifact"
export PYTHON_SHA256
export CRATE_SHA256
PYTHON_SHA256="$(sha256_file "$python_artifact")"
CRATE_SHA256="$(sha256_file "$crate_artifact")"

cat > "${mock_bin}/cargo" << 'MOCK'
#!/usr/bin/env bash
set -euo pipefail

if [[ "$*" == "metadata --no-deps --format-version=1" ]]; then
  cat <<'JSON'
{
  "packages": [
    {
      "name": "nautilus-core",
      "version": "1.2.3",
      "source": null,
      "publish": null
    }
  ]
}
JSON
  exit 0
fi

echo "unexpected cargo args: $*" >&2
exit 2
MOCK

cat > "${mock_bin}/uv" << 'MOCK'
#!/usr/bin/env bash
set -euo pipefail

mode="${PYPI_VERIFY_MOCK_MODE:-success}"
count=1
provenance_file=""
runs_pypi_attestations=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    pypi-attestations)
      runs_pypi_attestations=1
      shift
      ;;
    --provenance-file)
      provenance_file="${2:-}"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done

if [[ "$runs_pypi_attestations" -eq 1 &&
  "${PYPI_VERIFY_REQUIRE_PROVENANCE_FILE:-1}" == "1" ]]; then
  if [[ -z "$provenance_file" ]]; then
    echo "pypi-attestations verify was not passed --provenance-file" >&2
    exit 2
  fi
  if [[ ! -s "$provenance_file" ]]; then
    echo "pypi-attestations verify received a missing provenance file" >&2
    exit 2
  fi
fi

if [[ -n "${PYPI_VERIFY_ATTEMPT_FILE:-}" ]]; then
  count=0
  if [[ -f "$PYPI_VERIFY_ATTEMPT_FILE" ]]; then
    count="$(cat "$PYPI_VERIFY_ATTEMPT_FILE")"
  fi
  count=$((count + 1))
  printf '%s\n' "$count" > "$PYPI_VERIFY_ATTEMPT_FILE"
fi

case "$mode" in
  success)
    exit 0
    ;;
  transient-checkpoint)
    if [[ "$count" -lt 3 ]]; then
      echo "invalid log entry: checkpoint is not consistent yet" >&2
      exit 17
    fi
    echo "PyPI attestation checkpoint verification succeeded."
    ;;
  transient-rekor)
    if [[ "$count" -lt 2 ]]; then
      echo "Rekor inclusion proof is not available from the transparency log yet" >&2
      exit 18
    fi
    echo "PyPI attestation Rekor verification succeeded."
    ;;
  transient-tuf)
    if [[ "$count" -lt 2 ]]; then
      echo "TUF metadata is not consistent yet" >&2
      exit 19
    fi
    echo "PyPI attestation TUF verification succeeded."
    ;;
  provenance-mismatch)
    echo "provenance mismatch: repository did not match expected workflow environment" >&2
    exit 42
    ;;
  *)
    echo "unexpected PyPI verifier mock mode: $mode" >&2
    exit 2
    ;;
esac
MOCK

cat > "${mock_bin}/curl" << 'MOCK'
#!/usr/bin/env bash
set -euo pipefail

output=""
url=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --output)
      output=$2
      shift 2
      ;;
    --proto | --retry | --connect-timeout | --max-time | --header)
      shift 2
      ;;
    --tlsv1.2 | --silent | --show-error | --fail | --location | --retry-all-errors)
      shift
      ;;
    *)
      url=$1
      shift
      ;;
  esac
done

if [[ -z "$output" || -z "$url" ]]; then
  echo "curl mock missing output or URL" >&2
  exit 2
fi

record_attempt() {
  local name=$1
  local namespace="${CURL_ATTEMPT_NAMESPACE:-default}"
  local attempt_file="${CURL_ATTEMPT_DIR:?}/${namespace}-${name}"
  local count=0

  if [[ -f "$attempt_file" ]]; then
    count="$(cat "$attempt_file")"
  fi
  count=$((count + 1))
  printf '%s\n' "$count" > "$attempt_file"
  printf '%s\n' "$count"
}

write_crates_io_versions() {
  local mode="${CRATE_PUBLISHER_MODE:-manual}"

  case "$mode" in
    manual)
      cat > "$output" <<JSON
{"versions":[{"num":"1.2.3","checksum":"${CRATE_SHA256}","trustpub_data":null,"published_by":{"id":7,"login":"release-owner","name":"Release Owner"}}]}
JSON
      ;;
    trusted)
      cat > "$output" <<JSON
{"versions":[{"num":"1.2.3","checksum":"${CRATE_SHA256}","trustpub_data":{"provider":"github","repository":"nautechsystems/nautilus_trader","sha":"abc123"},"published_by":null}]}
JSON
      ;;
    *)
      echo "unexpected crate publisher mode: $mode" >&2
      exit 2
      ;;
  esac
}

case "$url" in
  https://pypi.org/pypi/nautilus_trader/json)
    cat > "$output" <<JSON
{"releases":{"1.2.3":[{"filename":"nautilus_trader-1.2.3.tar.gz","digests":{"sha256":"${PYTHON_SHA256}"},"url":"https://example.invalid/nautilus_trader-1.2.3.tar.gz"}]}}
JSON
    ;;
  https://example.invalid/nautilus_trader-1.2.3.tar.gz)
    cp "$PYTHON_ARTIFACT_FILE" "$output"
    ;;
  https://pypi.org/integrity/nautilus_trader/1.2.3/nautilus_trader-1.2.3.tar.gz/provenance*)
    pypi_provenance_attempt="$(record_attempt pypi-provenance)"
    if [[ "$pypi_provenance_attempt" -le "${PYPI_PROVENANCE_404_ATTEMPTS:-0}" ]]; then
      echo "curl: (22) The requested URL returned error: 404" >&2
      exit 22
    fi

    if [[ "${PYPI_PROVENANCE_MISMATCH:-0}" == "1" ]]; then
      cat > "$output" <<'JSON'
{"attestation_bundles":[{"publisher":{"kind":"GitHub","repository":"wrong/repo","workflow":"release.yml","environment":"staging"}}]}
JSON
      exit 0
    fi

    cat > "$output" <<'JSON'
{"attestation_bundles":[{"publisher":{"kind":"GitHub","repository":"nautechsystems/nautilus_trader","workflow":"build.yml","environment":"release"}}]}
JSON
    ;;
  https://crates.io/api/v1/crates/nautilus-core/versions)
    crates_api_attempt="$(record_attempt crates-api)"
    if [[ "$crates_api_attempt" -le "${CRATES_API_LAG_ATTEMPTS:-0}" ]]; then
      cat > "$output" <<'JSON'
{"versions":[]}
JSON
      exit 0
    fi

    write_crates_io_versions
    ;;
  https://static.crates.io/crates/nautilus-core/nautilus-core-1.2.3.crate)
    record_attempt static-crate > /dev/null
    cp "$CRATE_ARTIFACT_FILE" "$output"
    ;;
  https://index.crates.io/na/ut/nautilus-core)
    sparse_index_attempt="$(record_attempt sparse-index)"
    if [[ "$sparse_index_attempt" -le "${SPARSE_INDEX_LAG_ATTEMPTS:-0}" ]]; then
      cat > "$output" <<JSON
{"name":"nautilus-core","vers":"1.2.2","cksum":"${CRATE_SHA256}"}
JSON
      exit 0
    fi

    if [[ "${SPARSE_INDEX_CHECKSUM_MISMATCH:-0}" == "1" ]]; then
      cat > "$output" <<'JSON'
{"name":"nautilus-core","vers":"1.2.3","cksum":"bad-checksum"}
JSON
      exit 0
    fi

    cat > "$output" <<JSON
{"name":"nautilus-core","vers":"1.2.3","cksum":"${CRATE_SHA256}"}
JSON
    ;;
  *)
    echo "unexpected curl URL: $url" >&2
    exit 2
    ;;
esac
MOCK

chmod +x "${mock_bin}/cargo" "${mock_bin}/curl" "${mock_bin}/uv"

write_dist_manifest() {
  local asset_dir=$1

  mkdir -p "$asset_dir"
  jq -n \
    --arg sha256 "$PYTHON_SHA256" \
    '{
      artifacts: [
        {
          name: "nautilus_trader-1.2.3.tar.gz",
          sha256: $sha256
        }
      ]
    }' > "${asset_dir}/dist-manifest.json"
}

run_verifier() {
  local asset_dir=$1
  shift

  (
    cd "$repo_root"
    env \
      PATH="${mock_bin}:${PATH}" \
      TAG_NAME=v1.2.3 \
      GITHUB_SHA=abc123 \
      REGISTRY_PROPAGATION_TIMEOUT_SECONDS=1 \
      REGISTRY_PROPAGATION_POLL_SECONDS=1 \
      REGISTRY_PROPAGATION_SLEEP_COMMAND=: \
      RELEASE_VERIFICATION_SLEEP_COMMAND=: \
      PYPI_ATTESTATIONS_VERSION=0.0.0 \
      PYPI_ATTESTATION_VERIFY_ATTEMPTS=1 \
      PYPI_ATTESTATION_VERIFY_RETRY_DELAY_SECONDS=1 \
      PYPI_ATTESTATION_VERIFY_MAX_RETRY_DELAY_SECONDS=1 \
      "$@" \
      bash "${script_dir}/verify-published-registries.bash" "$asset_dir"
  )
}

curl_attempt_count() {
  local namespace=$1
  local name=$2

  cat "${CURL_ATTEMPT_DIR}/${namespace}-${name}"
}

assert_current_release_crates_manifest() {
  local asset_dir=$1

  if ! jq -e '
    .crates[0].name == "nautilus-core"
    and .crates[0].version == "1.2.3"
    and .crates[0].release_status == "current_release"
    and .crates[0].current_release_commit == true
    and .crates[0].manual_publish_exception == false
    and .crates[0].trusted_publishing.provider == "github"
    and .crates[0].trusted_publishing.repository == "nautechsystems/nautilus_trader"
    and .crates[0].trusted_publishing.sha == "abc123"
    and .crates[0].published_by == null
  ' "${asset_dir}/crates-manifest.json" > /dev/null; then
    fail "trusted-publishing crates-manifest.json entry was not recorded."
  fi
}

run_pypi_transient_case() {
  local mode=$1
  local expected_attempts=$2
  local success_text=$3
  local assets="${work_dir}/release-${mode}"
  local output="${work_dir}/${mode}-output.txt"
  local attempt_file="${work_dir}/${mode}-uv-attempts.txt"

  write_dist_manifest "$assets"
  rm -f "$attempt_file"
  run_verifier \
    "$assets" \
    CRATE_PUBLISHER_MODE=trusted \
    PYPI_VERIFY_MOCK_MODE="$mode" \
    PYPI_VERIFY_ATTEMPT_FILE="$attempt_file" \
    PYPI_ATTESTATION_VERIFY_ATTEMPTS=4 \
    CURL_ATTEMPT_NAMESPACE="$mode" \
    > "$output" 2>&1

  if [[ "$(cat "$attempt_file")" != "$expected_attempts" ]]; then
    fail "${mode} PyPI verifier case used the wrong attempt count."
  fi
  if [[ "$(curl_attempt_count "$mode" pypi-provenance)" != "$expected_attempts" ]]; then
    fail "${mode} PyPI verifier case did not refetch provenance on each attempt."
  fi
  if ! grep -q "$success_text" "$output"; then
    fail "${mode} PyPI verifier case did not print the verifier success output."
  fi
  if ! grep -q "retryable verification error" "$output"; then
    fail "${mode} PyPI verifier case did not report a retryable verification error."
  fi

  assert_current_release_crates_manifest "$assets"
}

run_pypi_transient_case \
  transient-checkpoint \
  3 \
  "PyPI attestation checkpoint verification succeeded."

run_pypi_transient_case \
  transient-rekor \
  2 \
  "PyPI attestation Rekor verification succeeded."

run_pypi_transient_case \
  transient-tuf \
  2 \
  "PyPI attestation TUF verification succeeded."

pypi_provenance_404_assets="${work_dir}/release-pypi-provenance-404"
write_dist_manifest "$pypi_provenance_404_assets"
pypi_provenance_404_attempts="${work_dir}/pypi-provenance-404-uv-attempts.txt"
pypi_provenance_404_output="${work_dir}/pypi-provenance-404-output.txt"
run_verifier \
  "$pypi_provenance_404_assets" \
  CRATE_PUBLISHER_MODE=trusted \
  PYPI_PROVENANCE_404_ATTEMPTS=2 \
  PYPI_VERIFY_ATTEMPT_FILE="$pypi_provenance_404_attempts" \
  PYPI_ATTESTATION_VERIFY_ATTEMPTS=4 \
  CURL_ATTEMPT_NAMESPACE=pypi-provenance-404 \
  > "$pypi_provenance_404_output" 2>&1

if [[ "$(curl_attempt_count pypi-provenance-404 pypi-provenance)" != "3" ]]; then
  fail "PyPI provenance 404 case should succeed on the third provenance fetch."
fi
if [[ "$(cat "$pypi_provenance_404_attempts")" != "1" ]]; then
  fail "PyPI provenance 404 case should run pypi-attestations verify once."
fi
if ! grep -q "retryable verification error" "$pypi_provenance_404_output"; then
  fail "PyPI provenance 404 case did not report a retryable verification error."
fi
assert_current_release_crates_manifest "$pypi_provenance_404_assets"

pypi_provenance_mismatch_assets="${work_dir}/release-pypi-provenance-mismatch"
write_dist_manifest "$pypi_provenance_mismatch_assets"
pypi_provenance_mismatch_attempts="${work_dir}/pypi-provenance-mismatch-uv-attempts.txt"
pypi_provenance_mismatch_output="${work_dir}/pypi-provenance-mismatch-output.txt"
set +e
run_verifier \
  "$pypi_provenance_mismatch_assets" \
  CRATE_PUBLISHER_MODE=trusted \
  PYPI_VERIFY_MOCK_MODE=provenance-mismatch \
  PYPI_VERIFY_ATTEMPT_FILE="$pypi_provenance_mismatch_attempts" \
  PYPI_ATTESTATION_VERIFY_ATTEMPTS=4 \
  > "$pypi_provenance_mismatch_output" 2>&1
pypi_provenance_mismatch_status=$?
set -e

if [[ "$pypi_provenance_mismatch_status" -eq 0 ]]; then
  fail "PyPI provenance mismatch verifier case should fail."
fi
if [[ "$(cat "$pypi_provenance_mismatch_attempts")" != "1" ]]; then
  fail "PyPI provenance mismatch verifier case should fail fast after one attempt."
fi
if ! grep -q "non-retryable verification error" "$pypi_provenance_mismatch_output"; then
  fail "PyPI provenance mismatch verifier case did not report a non-retryable error."
fi
if [[ -f "${pypi_provenance_mismatch_assets}/crates-manifest.json" ]]; then
  fail "PyPI provenance mismatch verifier case should fail before crates verification."
fi

pypi_provenance_endpoint_mismatch_assets="${work_dir}/release-pypi-provenance-endpoint-mismatch"
write_dist_manifest "$pypi_provenance_endpoint_mismatch_assets"
pypi_provenance_endpoint_mismatch_attempts="${work_dir}/pypi-provenance-endpoint-uv-attempts.txt"
pypi_provenance_endpoint_mismatch_output="${work_dir}/pypi-provenance-endpoint-output.txt"
set +e
run_verifier \
  "$pypi_provenance_endpoint_mismatch_assets" \
  CRATE_PUBLISHER_MODE=trusted \
  PYPI_PROVENANCE_MISMATCH=1 \
  PYPI_VERIFY_ATTEMPT_FILE="$pypi_provenance_endpoint_mismatch_attempts" \
  CURL_ATTEMPT_NAMESPACE=pypi-provenance-endpoint-mismatch \
  > "$pypi_provenance_endpoint_mismatch_output" 2>&1
pypi_provenance_endpoint_mismatch_status=$?
set -e

if [[ "$pypi_provenance_endpoint_mismatch_status" -eq 0 ]]; then
  fail "PyPI provenance endpoint mismatch case should fail."
fi
if [[ "$(curl_attempt_count pypi-provenance-endpoint-mismatch pypi-provenance)" != "1" ]]; then
  fail "PyPI provenance endpoint mismatch case should fail fast after one provenance fetch."
fi
if [[ -f "$pypi_provenance_endpoint_mismatch_attempts" ]]; then
  fail "PyPI provenance endpoint mismatch case should not run pypi-attestations verify."
fi
if ! grep -q "has no matching publisher identity" \
  "$pypi_provenance_endpoint_mismatch_output"; then
  fail "PyPI provenance endpoint mismatch case did not report the publisher mismatch."
fi
if [[ -f "${pypi_provenance_endpoint_mismatch_assets}/crates-manifest.json" ]]; then
  fail "PyPI provenance endpoint mismatch case should fail before crates verification."
fi

crates_api_lag_assets="${work_dir}/release-crates-api-lag"
write_dist_manifest "$crates_api_lag_assets"
crates_api_lag_output="${work_dir}/crates-api-lag-output.txt"
run_verifier \
  "$crates_api_lag_assets" \
  CRATE_PUBLISHER_MODE=trusted \
  CRATES_API_LAG_ATTEMPTS=2 \
  CURL_ATTEMPT_NAMESPACE=crates-api-lag \
  REGISTRY_PROPAGATION_TIMEOUT_SECONDS=60 \
  > "$crates_api_lag_output" 2>&1

if [[ "$(curl_attempt_count crates-api-lag crates-api)" != "3" ]]; then
  fail "crates.io API lag case should succeed on the third API attempt."
fi
if ! grep -q "Waiting for crates.io API version nautilus-core 1.2.3 to propagate" \
  "$crates_api_lag_output"; then
  fail "crates.io API lag case did not report registry propagation polling."
fi
assert_current_release_crates_manifest "$crates_api_lag_assets"

sparse_index_lag_assets="${work_dir}/release-sparse-index-lag"
write_dist_manifest "$sparse_index_lag_assets"
sparse_index_lag_output="${work_dir}/sparse-index-lag-output.txt"
run_verifier \
  "$sparse_index_lag_assets" \
  CRATE_PUBLISHER_MODE=trusted \
  SPARSE_INDEX_LAG_ATTEMPTS=2 \
  CURL_ATTEMPT_NAMESPACE=sparse-index-lag \
  REGISTRY_PROPAGATION_TIMEOUT_SECONDS=60 \
  > "$sparse_index_lag_output" 2>&1

if [[ "$(curl_attempt_count sparse-index-lag sparse-index)" != "3" ]]; then
  fail "sparse-index lag case should succeed on the third sparse-index attempt."
fi
if ! grep -q "Waiting for sparse index entry nautilus-core 1.2.3 to propagate" \
  "$sparse_index_lag_output"; then
  fail "sparse-index lag case did not report registry propagation polling."
fi
assert_current_release_crates_manifest "$sparse_index_lag_assets"

sparse_index_checksum_mismatch_assets="${work_dir}/release-sparse-index-checksum-mismatch"
write_dist_manifest "$sparse_index_checksum_mismatch_assets"
sparse_index_checksum_mismatch_output="${work_dir}/sparse-index-checksum-mismatch-output.txt"
set +e
run_verifier \
  "$sparse_index_checksum_mismatch_assets" \
  CRATE_PUBLISHER_MODE=trusted \
  SPARSE_INDEX_CHECKSUM_MISMATCH=1 \
  CURL_ATTEMPT_NAMESPACE=sparse-index-checksum-mismatch \
  > "$sparse_index_checksum_mismatch_output" 2>&1
sparse_index_checksum_mismatch_status=$?
set -e

if [[ "$sparse_index_checksum_mismatch_status" -eq 0 ]]; then
  fail "sparse-index checksum mismatch case should fail."
fi
if [[ "$(curl_attempt_count sparse-index-checksum-mismatch sparse-index)" != "1" ]]; then
  fail "sparse-index checksum mismatch case should fail fast after one sparse-index attempt."
fi
if ! grep -q "Sparse index checksum mismatch for nautilus-core 1.2.3" \
  "$sparse_index_checksum_mismatch_output"; then
  fail "sparse-index checksum mismatch case did not report the checksum mismatch."
fi

no_exception_assets="${work_dir}/release-no-exception"
write_dist_manifest "$no_exception_assets"
no_exception_output="${work_dir}/no-exception-output.txt"
set +e
run_verifier "$no_exception_assets" > "$no_exception_output" 2>&1
no_exception_status=$?
set -e

if [[ "$no_exception_status" -eq 0 ]]; then
  fail "manual token publish without an exception should fail."
fi
if ! grep -q "Expected trusted publishing for nautilus-core" "$no_exception_output"; then
  fail "manual token publish without an exception did not report the trusted-publishing error."
fi

invalid_exception_assets="${work_dir}/release-invalid-exception"
write_dist_manifest "$invalid_exception_assets"
invalid_exception_output="${work_dir}/invalid-exception-output.txt"
set +e
run_verifier \
  "$invalid_exception_assets" \
  CRATES_IO_MANUAL_PUBLISH_EXCEPTIONS=bad-entry \
  > "$invalid_exception_output" 2>&1
invalid_exception_status=$?
set -e

if [[ "$invalid_exception_status" -eq 0 ]]; then
  fail "invalid manual token publish exception should fail."
fi
if ! grep -q "Invalid CRATES_IO_MANUAL_PUBLISH_EXCEPTIONS entry" "$invalid_exception_output"; then
  fail "invalid manual token publish exception did not report the invalid entry."
fi

wrong_version_assets="${work_dir}/release-wrong-version"
write_dist_manifest "$wrong_version_assets"
wrong_version_output="${work_dir}/wrong-version-output.txt"
set +e
run_verifier \
  "$wrong_version_assets" \
  CRATES_IO_MANUAL_PUBLISH_EXCEPTIONS=nautilus-core@1.2.4 \
  > "$wrong_version_output" 2>&1
wrong_version_status=$?
set -e

if [[ "$wrong_version_status" -eq 0 ]]; then
  fail "wrong-version manual token publish exception should fail."
fi
if ! grep -q "Expected trusted publishing for nautilus-core" "$wrong_version_output"; then
  fail "wrong-version manual token publish exception did not report the trusted-publishing error."
fi

manual_exception_assets="${work_dir}/release-manual-exception"
write_dist_manifest "$manual_exception_assets"
manual_exception_output="${work_dir}/manual-exception-output.txt"
run_verifier \
  "$manual_exception_assets" \
  CRATES_IO_MANUAL_PUBLISH_EXCEPTIONS=nautilus-core@1.2.3 \
  > "$manual_exception_output" 2>&1

if ! jq -e '
  .crates[0].name == "nautilus-core"
  and .crates[0].version == "1.2.3"
  and .crates[0].release_status == "manual_token_publish"
  and .crates[0].current_release_commit == false
  and .crates[0].manual_publish_exception == true
  and .crates[0].trusted_publishing == null
  and .crates[0].published_by.login == "release-owner"
' "${manual_exception_assets}/crates-manifest.json" > /dev/null; then
  fail "manual token publish exception was not recorded in crates-manifest.json."
fi

unused_exception_assets="${work_dir}/release-unused-exception"
write_dist_manifest "$unused_exception_assets"
unused_exception_output="${work_dir}/unused-exception-output.txt"
set +e
run_verifier \
  "$unused_exception_assets" \
  CRATES_IO_MANUAL_PUBLISH_EXCEPTIONS="nautilus-core@1.2.3 unused-crate@1.2.3" \
  > "$unused_exception_output" 2>&1
unused_exception_status=$?
set -e

if [[ "$unused_exception_status" -eq 0 ]]; then
  fail "unused manual token publish exception should fail."
fi
if ! grep -q "Unused CRATES_IO_MANUAL_PUBLISH_EXCEPTIONS entries" "$unused_exception_output"; then
  fail "unused manual token publish exception did not report the unused entry."
fi

echo "verify published registries crates tests passed."
