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

exit 0
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

case "$url" in
  https://pypi.org/pypi/nautilus_trader/json)
    cat > "$output" <<JSON
{"releases":{"1.2.3":[{"filename":"nautilus_trader-1.2.3.tar.gz","digests":{"sha256":"${PYTHON_SHA256}"},"url":"https://example.invalid/nautilus_trader-1.2.3.tar.gz"}]}}
JSON
    ;;
  https://example.invalid/nautilus_trader-1.2.3.tar.gz)
    cp "$PYTHON_ARTIFACT_FILE" "$output"
    ;;
  https://pypi.org/integrity/nautilus_trader/1.2.3/nautilus_trader-1.2.3.tar.gz/provenance)
    cat > "$output" <<'JSON'
{"attestation_bundles":[{"publisher":{"kind":"GitHub","repository":"nautechsystems/nautilus_trader","workflow":"build.yml","environment":"release"}}]}
JSON
    ;;
  https://crates.io/api/v1/crates/nautilus-core/versions)
    cat > "$output" <<JSON
{"versions":[{"num":"1.2.3","checksum":"${CRATE_SHA256}","trustpub_data":null,"published_by":{"id":7,"login":"release-owner","name":"Release Owner"}}]}
JSON
    ;;
  https://static.crates.io/crates/nautilus-core/nautilus-core-1.2.3.crate)
    cp "$CRATE_ARTIFACT_FILE" "$output"
    ;;
  https://index.crates.io/na/ut/nautilus-core)
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
      PYPI_ATTESTATION_VERIFY_ATTEMPTS=1 \
      PYPI_ATTESTATION_VERIFY_RETRY_DELAY_SECONDS=1 \
      PYPI_ATTESTATION_VERIFY_MAX_RETRY_DELAY_SECONDS=1 \
      "$@" \
      bash "${script_dir}/verify-published-registries.bash" "$asset_dir"
  )
}

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
