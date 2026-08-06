#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/../.." && pwd)"
work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

fail() {
  echo "::error::$1" >&2
  exit 1
}

assert_file() {
  if [[ ! -f "$1" ]]; then
    fail "Expected file $1"
  fi
}

assert_absent() {
  if [[ -e "$1" ]]; then
    fail "Expected $1 to be absent"
  fi
}

assert_line() {
  if ! grep -Fxq "$2" "$1"; then
    fail "Expected exact line '$2' in $1"
  fi
}

run_expect_failure() {
  local output=$1
  shift
  local status

  set +e
  "$@" > "$output" 2>&1
  status=$?
  set -e
  if [[ "$status" -eq 0 ]]; then
    fail "Expected command to fail: $*"
  fi
}

test_policy_and_version() {
  local case_dir="${work_dir}/policy"
  local output="${case_dir}/output"
  local base_version
  local version

  base_version="$(awk -F '"' '/^version = / { print $2; exit }' "${repo_root}/python/pyproject.toml" |
    sed -E 's/(\.dev[0-9]{8}(\+[0-9]+)?|a[0-9]{8})$//')"
  version="${base_version}.dev20260803+451"

  mkdir -p "$case_dir"
  [[ "$(bash "${repo_root}/scripts/ci/publish-wheels-policy.bash" pull_request test-ci)" == "none" ]] ||
    fail "Pull requests must not publish"
  [[ "$(bash "${repo_root}/scripts/ci/publish-wheels-policy.bash" push develop)" == "development" ]] ||
    fail "Develop push must use development publication"
  [[ "$(bash "${repo_root}/scripts/ci/publish-wheels-policy.bash" push test-ci)" == "none" ]] ||
    fail "Test-ci push must not publish"
  [[ "$(bash "${repo_root}/scripts/ci/release-version-policy.bash" 2.0.0)" == "release" ]] ||
    fail "Final version must use release policy"
  [[ "$(bash "${repo_root}/scripts/ci/release-version-policy.bash" 2.0.0rc3)" == "prerelease" ]] ||
    fail "Release candidate must use prerelease policy"
  [[ "$(bash "${repo_root}/scripts/ci/release-version-policy.bash" 2.0.0a1)" == "prerelease" ]] ||
    fail "Alpha version must use prerelease policy"
  [[ "$(bash "${repo_root}/scripts/ci/release-version-policy.bash" 2.0.0b2)" == "prerelease" ]] ||
    fail "Beta version must use prerelease policy"
  run_expect_failure "$output" bash "${repo_root}/scripts/ci/release-version-policy.bash" \
    2.0.0rc3.dev20260803

  (
    cd "$repo_root"
    EVENT_NAME=push \
      REF_NAME=test-ci \
      GITHUB_RUN_NUMBER=451 \
      PUBLISH_DATE=20260803 \
      GITHUB_OUTPUT="$output" \
      bash scripts/ci/plan-wheel-publication.bash
  )
  assert_line "$output" "publish_r2=false"
  assert_line "$output" "publish_development=false"
  assert_line "$output" "publish_environment="
  assert_line "$output" "wheel_matrix=none"
  assert_line "$output" "wheel_version="

  : > "$output"
  (
    cd "$repo_root"
    EVENT_NAME=push \
      REF_NAME=develop \
      GITHUB_RUN_NUMBER=451 \
      PUBLISH_DATE=20260803 \
      GITHUB_OUTPUT="$output" \
      bash scripts/ci/plan-wheel-publication.bash
  )
  assert_line "$output" "publish_r2=true"
  assert_line "$output" "publish_development=true"
  assert_line "$output" "publish_environment=r2-develop"
  assert_line "$output" "wheel_matrix=development"
  assert_line "$output" "wheel_version=${version}"

  mkdir -p "${case_dir}/package"
  cp "${repo_root}/python/pyproject.toml" "${case_dir}/package/pyproject.toml"
  (
    cd "${case_dir}/package"
    GITHUB_REF_NAME=develop \
      PUBLISH_WHEEL_VERSION="$version" \
      bash "${repo_root}/scripts/ci/update-pyproject-version.sh"
  )
  assert_line "${case_dir}/package/pyproject.toml" "version = \"${version}\""
}

test_security_gate() {
  local case_dir="${work_dir}/security"
  local date_bin="${case_dir}/date-bin"
  local mock_bin="${case_dir}/bin"
  local output="${case_dir}/output"

  mkdir -p "$mock_bin"
  cat > "${mock_bin}/gh" << 'MOCK'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "${MOCK_GH_ARGS_LOG:?}"
if [[ -n "${MOCK_GH_RESPONSES:-}" ]]; then
  count="$(cat "${MOCK_GH_COUNT:?}")"
  count=$((count + 1))
  echo "$count" > "${MOCK_GH_COUNT:?}"
  sed -n "${count}p" "$MOCK_GH_RESPONSES"
  exit 0
fi
printf '%s\n' "${MOCK_GH_RESPONSE:-}"
MOCK
  cat > "${mock_bin}/sleep" << 'MOCK'
#!/usr/bin/env bash
set -euo pipefail
exit 0
MOCK
  chmod +x "${mock_bin}/gh" "${mock_bin}/sleep"

  run_audit_check() {
    PATH="${mock_bin}:$PATH" \
      MOCK_GH_ARGS_LOG="${case_dir}/gh-args" \
      MOCK_GH_RESPONSE="${1:-}" \
      GITHUB_REPOSITORY=nautechsystems/nautilus_trader \
      GITHUB_SHA=1111111111111111111111111111111111111111 \
      GITHUB_REF_NAME=develop \
      GITHUB_EVENT_NAME="${2:-push}" \
      SECURITY_AUDIT_TIMEOUT_SECONDS=0 \
      SECURITY_AUDIT_POLL_SECONDS=1 \
      bash "${repo_root}/scripts/ci/check-security-audit-result.sh"
  }

  run_audit_check '123|completed|success|https://example.invalid/run|2026-08-03T00:00:00Z'
  grep -Fq 'branch=develop' "${case_dir}/gh-args" || fail "Audit query did not bind the branch"
  grep -Fq 'event=push' "${case_dir}/gh-args" || fail "Audit query did not bind the push event"
  grep -Fq 'head_sha=1111111111111111111111111111111111111111' "${case_dir}/gh-args" ||
    fail "Audit query did not bind the commit SHA"

  printf '%s\n' \
    '123|in_progress||https://example.invalid/run|2026-08-03T00:00:00Z' \
    '123|completed|success|https://example.invalid/run|2026-08-03T00:00:00Z' \
    > "${case_dir}/responses"
  echo 0 > "${case_dir}/count"
  PATH="${mock_bin}:$PATH" \
    MOCK_GH_ARGS_LOG="${case_dir}/gh-args" \
    MOCK_GH_RESPONSES="${case_dir}/responses" \
    MOCK_GH_COUNT="${case_dir}/count" \
    GITHUB_REPOSITORY=nautechsystems/nautilus_trader \
    GITHUB_SHA=1111111111111111111111111111111111111111 \
    GITHUB_REF_NAME=develop \
    GITHUB_EVENT_NAME=push \
    SECURITY_AUDIT_TIMEOUT_SECONDS=5 \
    SECURITY_AUDIT_POLL_SECONDS=1 \
    bash "${repo_root}/scripts/ci/check-security-audit-result.sh"
  [[ "$(cat "${case_dir}/count")" == "2" ]] || fail "Audit checker did not wait for completion"

  run_expect_failure "$output" run_audit_check ''
  grep -Fq "last state was missing" "$output" || fail "Missing audit must time out closed"
  run_expect_failure "$output" run_audit_check \
    '123|in_progress||https://example.invalid/run|2026-08-03T00:00:00Z'
  grep -Fq "in_progress/none" "$output" || fail "Incomplete audit state was not preserved"
  run_expect_failure "$output" run_audit_check \
    '123|completed|neutral|https://example.invalid/run|2026-08-03T00:00:00Z'
  run_expect_failure "$output" run_audit_check \
    '123|completed|failure|https://example.invalid/run|2026-08-03T00:00:00Z'
  run_expect_failure "$output" run_audit_check \
    '123|completed|success|https://example.invalid/run|2026-08-03T00:00:00Z' pull_request

  : > "$output"
  EVENT_NAME=push \
    FORCE_SECURITY_AUDIT=true \
    SECURITY_GATE_OVERRIDE=2999-01-01T00:00:00Z \
    GITHUB_OUTPUT="$output" \
    bash "${repo_root}/scripts/ci/security-audit-gate.sh"
  assert_line "$output" "audit_needed=false"
  assert_line "$output" "override_active=true"

  : > "$output"
  EVENT_NAME=push \
    FORCE_SECURITY_AUDIT=true \
    SECURITY_GATE_OVERRIDE=2000-01-01T00:00:00Z \
    GITHUB_OUTPUT="$output" \
    bash "${repo_root}/scripts/ci/security-audit-gate.sh"
  assert_line "$output" "audit_needed=true"
  assert_line "$output" "override_active=false"

  mkdir -p "$date_bin"
  cat > "${date_bin}/date" << 'MOCK'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$*" == "-u +%s" ]]; then
  echo 100
elif [[ "${1:-}" == "-u" && "${2:-}" == "-d" ]]; then
  exit 1
elif [[ "$*" == "-j -u -f %Y-%m-%dT%H:%M:%SZ 2999-01-01T00:00:00Z +%s" ]]; then
  echo 200
else
  exit 1
fi
MOCK
  chmod +x "${date_bin}/date"
  : > "$output"
  PATH="${date_bin}:$PATH" \
    EVENT_NAME=push \
    FORCE_SECURITY_AUDIT=true \
    SECURITY_GATE_OVERRIDE=2999-01-01T00:00:00Z \
    GITHUB_OUTPUT="$output" \
    bash "${repo_root}/scripts/ci/security-audit-gate.sh"
  assert_line "$output" "audit_needed=false"
  assert_line "$output" "override_active=true"

  : > "$output"
  EVENT_NAME=pull_request \
    FORCE_SECURITY_AUDIT=true \
    PR_BASE_REF=develop \
    PR_HEAD_SHA="$(git -C "$repo_root" rev-parse HEAD)" \
    SECURITY_GATE_OVERRIDE=2999-01-01T00:00:00Z \
    GITHUB_OUTPUT="$output" \
    bash "${repo_root}/scripts/ci/security-audit-gate.sh"
  assert_line "$output" "override_active=false"
}

test_sha256_portability() {
  local case_dir="${work_dir}/sha256-portability"
  local fallback_bin="${case_dir}/bin"
  local expected
  local actual

  mkdir -p "$fallback_bin"
  ln -s "$(command -v awk)" "${fallback_bin}/awk"
  ln -s "$(command -v shasum)" "${fallback_bin}/shasum"
  printf 'portable sha256\n' > "${case_dir}/input"
  expected="$(shasum -a 256 "${case_dir}/input" | awk '{ print $1 }')"
  actual="$(
    PATH="$fallback_bin" /bin/bash \
      "${repo_root}/scripts/ci/publish-wheels-sha256.bash" "${case_dir}/input"
  )"
  [[ "$actual" == "$expected" ]] || fail "shasum fallback produced ${actual}, expected ${expected}"
}

create_development_wheels() {
  local dist=$1
  local version=$2

  mkdir -p "$dist"
  printf 'cp312-%s\n' "$version" > "${dist}/nautilus_trader-${version}-cp312-cp312-manylinux_2_34_x86_64.whl"
  printf 'cp313-%s\n' "$version" > "${dist}/nautilus_trader-${version}-cp313-cp313-manylinux_2_34_x86_64.whl"
  printf 'cp314-%s\n' "$version" > "${dist}/nautilus_trader-${version}-cp314-cp314-manylinux_2_34_x86_64.whl"
}

create_full_wheels() {
  local dist=$1
  local version=$2

  mkdir -p "$dist"
  for python_tag in cp312 cp313 cp314; do
    printf '%s-linux-x86\n' "$python_tag" > "${dist}/nautilus_trader-${version}-${python_tag}-${python_tag}-manylinux_2_34_x86_64.whl"
    printf '%s-linux-arm\n' "$python_tag" > "${dist}/nautilus_trader-${version}-${python_tag}-${python_tag}-manylinux_2_35_aarch64.whl"
    printf '%s-macos\n' "$python_tag" > "${dist}/nautilus_trader-${version}-${python_tag}-${python_tag}-macosx_11_0_arm64.whl"
    printf '%s-windows\n' "$python_tag" > "${dist}/nautilus_trader-${version}-${python_tag}-${python_tag}-win_amd64.whl"
  done
}

test_artifact_matrix() {
  local version="2.0.0rc3.dev20260803+451"
  local valid="${work_dir}/matrix-valid"
  local missing="${work_dir}/matrix-missing"
  local mixed="${work_dir}/matrix-mixed"
  local extra="${work_dir}/matrix-extra"
  local nightly="${work_dir}/matrix-nightly"
  local stable="${work_dir}/matrix-stable"
  local output="${work_dir}/matrix-output"

  create_development_wheels "${valid}/dist" "$version"
  (
    cd "$valid"
    PUBLISH_WHEEL_VERSION="$version" PUBLISH_WHEEL_MATRIX=development \
      bash "${repo_root}/scripts/ci/validate-wheel-artifacts.bash" manifest
  )
  [[ "$(wc -l < "${valid}/manifest" | tr -d ' ')" == "3" ]] || fail "Valid matrix must have three wheels"

  create_development_wheels "${missing}/dist" "$version"
  rm "${missing}/dist/nautilus_trader-${version}-cp314-cp314-manylinux_2_34_x86_64.whl"
  run_expect_failure "$output" bash -c \
    "cd '$missing' && PUBLISH_WHEEL_VERSION='$version' PUBLISH_WHEEL_MATRIX=development bash '$repo_root/scripts/ci/validate-wheel-artifacts.bash' manifest"

  create_development_wheels "${mixed}/dist" "$version"
  mv "${mixed}/dist/nautilus_trader-${version}-cp314-cp314-manylinux_2_34_x86_64.whl" \
    "${mixed}/dist/nautilus_trader-2.0.0rc3.dev20260803+452-cp314-cp314-manylinux_2_34_x86_64.whl"
  run_expect_failure "$output" bash -c \
    "cd '$mixed' && PUBLISH_WHEEL_VERSION='$version' PUBLISH_WHEEL_MATRIX=development bash '$repo_root/scripts/ci/validate-wheel-artifacts.bash' manifest"

  create_development_wheels "${extra}/dist" "$version"
  printf 'extra\n' > "${extra}/dist/nautilus_trader-${version}-cp311-cp311-manylinux_2_34_x86_64.whl"
  run_expect_failure "$output" bash -c \
    "cd '$extra' && PUBLISH_WHEEL_VERSION='$version' PUBLISH_WHEEL_MATRIX=development bash '$repo_root/scripts/ci/validate-wheel-artifacts.bash' manifest"

  create_full_wheels "${nightly}/dist" "2.0.0rc3.dev20260803"
  (
    cd "$nightly"
    PUBLISH_WHEEL_VERSION=2.0.0rc3.dev20260803 PUBLISH_WHEEL_MATRIX=nightly \
      bash "${repo_root}/scripts/ci/validate-wheel-artifacts.bash" manifest
  )
  [[ "$(wc -l < "${nightly}/manifest" | tr -d ' ')" == "12" ]] ||
    fail "Valid nightly matrix must have twelve wheels"

  create_full_wheels "${stable}/dist" "2.0.0rc3"
  (
    cd "$stable"
    PUBLISH_WHEEL_VERSION=2.0.0rc3 PUBLISH_WHEEL_MATRIX=stable \
      bash "${repo_root}/scripts/ci/validate-wheel-artifacts.bash" manifest
  )
  [[ "$(wc -l < "${stable}/manifest" | tr -d ' ')" == "12" ]] ||
    fail "Valid stable matrix must have twelve wheels"

  run_expect_failure "$output" bash -c \
    "cd '$stable' && PUBLISH_WHEEL_VERSION='2.0.0rc3.dev20260803' PUBLISH_WHEEL_MATRIX=stable bash '$repo_root/scripts/ci/validate-wheel-artifacts.bash' manifest"
}

create_r2_mocks() {
  local mock_bin=$1

  mkdir -p "$mock_bin"
  cat > "${mock_bin}/aws" << 'MOCK'
#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == "--version" ]]; then
  echo "aws-cli/mock"
  exit 0
fi
if [[ "${1:-}" != "s3" ]]; then
  echo "Unexpected aws command: $*" >&2
  exit 91
fi

operation=$2
source_path=${3:-}
destination_path=${4:-}
root="${MOCK_R2_ROOT:?}"
expected_base="s3://${CLOUDFLARE_R2_BUCKET_NAME:?}/${CLOUDFLARE_R2_PREFIX:?}/"

local_path() {
  local uri=$1
  if [[ "$uri" != "${expected_base}"* ]]; then
    echo "Refusing unexpected R2 path ${uri}" >&2
    exit 92
  fi
  printf '%s/%s\n' "$root" "${uri#"$expected_base"}"
}

case "$operation" in
  ls)
    if [[ "$source_path" != "$expected_base" && "$source_path" != "${expected_base}index.html" ]]; then
      echo "Refusing unexpected list path ${source_path}" >&2
      exit 92
    fi
    if [[ "$source_path" == "${expected_base}index.html" ]]; then
      [[ -f "${root}/index.html" ]] || exit 1
      printf '2026-08-03 00:00:00 %s index.html\n' "$(wc -c < "${root}/index.html")"
      exit 0
    fi
    for file in "$root"/*; do
      [[ -f "$file" ]] || continue
      printf '2026-08-03 00:00:00 %s %s\n' "$(wc -c < "$file")" "$(basename "$file")"
    done
    ;;
  cp)
    if [[ "$source_path" == s3://* ]]; then
      /bin/cp "$(local_path "$source_path")" "$destination_path"
    elif [[ "$destination_path" == s3://* ]]; then
      target="$(local_path "$destination_path")"
      if [[ "$destination_path" == */ ]]; then
        target="${target}$(basename "$source_path")"
      fi
      if [[ -n "${MOCK_R2_FAIL_UPLOAD:-}" && "$(basename "$target")" == "$MOCK_R2_FAIL_UPLOAD" ]]; then
        exit 97
      fi
      /bin/cp "$source_path" "$target"
    else
      echo "Expected one R2 path for cp" >&2
      exit 93
    fi
    ;;
  rm)
    target="$(local_path "$source_path")"
    if [[ -n "${MOCK_R2_FAIL_DELETE:-}" && "$(basename "$target")" == "$MOCK_R2_FAIL_DELETE" ]]; then
      exit 94
    fi
    /bin/rm "$target"
    ;;
  *)
    echo "Unexpected s3 operation ${operation}" >&2
    exit 95
    ;;
esac
MOCK

  cat > "${mock_bin}/curl" << 'MOCK'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" != "-fsSLo" || "${3:-}" != "${PUBLISH_WHEELS_INDEX_URL%/}/nautilus-trader/" ]]; then
  echo "Unexpected curl invocation: $*" >&2
  exit 96
fi
/bin/cp "${MOCK_R2_ROOT:?}/index.html" "$2"
MOCK

  cat > "${mock_bin}/uv" << 'MOCK'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "${MOCK_UV_LOG:?}"
if [[ "${1:-}" == "venv" ]]; then
  destination="${@: -1}"
  mkdir -p "${destination}/bin"
fi
MOCK

  cat > "${mock_bin}/sleep" << 'MOCK'
#!/usr/bin/env bash
set -euo pipefail
exit 0
MOCK

  chmod +x "${mock_bin}/aws" "${mock_bin}/curl" "${mock_bin}/uv" "${mock_bin}/sleep"
}

write_index() {
  local bucket=$1
  local index="${bucket}/index.html"

  {
    echo '<!DOCTYPE html>'
    echo '<html><head><title>NautilusTrader Packages</title></head>'
    echo '<body><h1>Packages for nautilus_trader</h1>'
    for path in "${bucket}"/nautilus_trader-*.whl; do
      [[ -f "$path" ]] || continue
      filename="$(basename "$path")"
      hash="$(bash "${repo_root}/scripts/ci/publish-wheels-sha256.bash" "$path")"
      printf '<a href="%s#sha256=%s">%s</a><br>\n' "$filename" "$hash" "$filename"
    done
    echo '</body></html>'
  } > "$index"
}

prepare_transaction() {
  local case_dir=$1
  local version=$2
  local bucket="${case_dir}/r2"

  mkdir -p "$bucket"
  create_development_wheels "${case_dir}/dist" "$version"
  printf 'old-development\n' > "${bucket}/nautilus_trader-2.0.0rc3.dev20260802+400-cp313-cp313-manylinux_2_34_x86_64.whl"
  printf 'release\n' > "${bucket}/nautilus_trader-2.0.0rc3-cp313-cp313-manylinux_2_34_x86_64.whl"
  printf 'rc-nightly\n' > "${bucket}/nautilus_trader-2.0.0rc3.dev20260802-cp313-cp313-manylinux_2_34_x86_64.whl"
  printf 'alpha-nightly\n' > "${bucket}/nautilus_trader-2.0.0a20260802-cp313-cp313-manylinux_2_34_x86_64.whl"
  printf 'unrelated\n' > "${bucket}/other-object.txt"
  write_index "$bucket"
}

run_transaction() {
  local case_dir=$1
  local version=$2
  local fail_delete=${3:-}
  local fail_upload=${4:-}
  local mock_bin="${work_dir}/r2-bin"
  local prefix="simple/test-nautilus-trader"

  create_r2_mocks "$mock_bin"
  : > "${case_dir}/uv.log"
  (
    cd "$case_dir"
    export AWS_ACCESS_KEY_ID=''
    export AWS_SECRET_ACCESS_KEY=''
    export AWS_SESSION_TOKEN=''
    export AWS_SHARED_CREDENTIALS_FILE=/dev/null
    export AWS_CONFIG_FILE=/dev/null
    export MOCK_R2_ROOT="${case_dir}/r2"
    export MOCK_R2_FAIL_DELETE="$fail_delete"
    export MOCK_R2_FAIL_UPLOAD="$fail_upload"
    export MOCK_UV_LOG="${case_dir}/uv.log"
    export CLOUDFLARE_R2_BUCKET_NAME=test-bucket
    export CLOUDFLARE_R2_PREFIX="$prefix"
    export CLOUDFLARE_R2_URL=https://r2.invalid
    export PUBLISH_WHEELS_INDEX_URL=https://packages.invalid/simple
    export PUBLISH_WHEEL_MATRIX=development
    export PUBLISH_WHEEL_VERSION="$version"
    export PUBLISH_WHEELS_MANIFEST="${case_dir}/manifest"
    export PUBLISH_WHEELS_DELETE_MANIFEST="${case_dir}/delete"
    export PUBLISH_WHEELS_INDEX_FILE="${case_dir}/generated-index.html"

    bash "${repo_root}/scripts/ci/validate-wheel-artifacts.bash" "${case_dir}/manifest"
    PATH="${mock_bin}:$PATH" bash "${repo_root}/scripts/ci/publish-wheels-r2.bash"
  )
}

test_stable_retention_plan() {
  local case_dir="${work_dir}/stable-retention"
  local mock_bin="${work_dir}/r2-bin"
  local prefix="simple/test-nautilus-trader"

  mkdir -p "${case_dir}/r2"
  create_r2_mocks "$mock_bin"
  printf 'older-release\n' > \
    "${case_dir}/r2/nautilus_trader-1.231.0-cp313-cp313-manylinux_2_34_x86_64.whl"
  printf 'development\n' > \
    "${case_dir}/r2/nautilus_trader-2.0.0rc3.dev20260803+451-cp313-cp313-manylinux_2_34_x86_64.whl"

  PATH="${mock_bin}:$PATH" \
    MOCK_R2_ROOT="${case_dir}/r2" \
    CLOUDFLARE_R2_BUCKET_NAME=test-bucket \
    CLOUDFLARE_R2_PREFIX="$prefix" \
    CLOUDFLARE_R2_URL=https://r2.invalid \
    PUBLISH_WHEELS_DELETE_MANIFEST="${case_dir}/delete" \
    PUBLISH_WHEEL_MATRIX=stable \
    PUBLISH_WHEEL_VERSION=2.0.0rc3 \
    bash "${repo_root}/scripts/ci/publish-wheels-r2-remove-old-wheels.sh" plan

  if [[ -s "${case_dir}/delete" ]]; then
    fail "Stable publication must not delete existing wheels"
  fi
}

test_orphan_purge_index_order() {
  local case_dir="${work_dir}/orphan-purge"
  local mock_bin="${work_dir}/r2-bin"
  local orphan="nautilus_trader-1.221.0.dev20251026+11610-cp311-cp311-macosx_15_0_arm64.whl"
  local output="${case_dir}/output"
  local prefix="simple/test-nautilus-trader"

  mkdir -p "${case_dir}/r2"
  create_r2_mocks "$mock_bin"
  printf 'orphan\n' > "${case_dir}/r2/${orphan}"
  printf 'release\n' > "${case_dir}/r2/nautilus_trader-2.0.0-cp313-cp313-manylinux_2_34_x86_64.whl"
  write_index "${case_dir}/r2"

  run_expect_failure "$output" env \
    PATH="${mock_bin}:$PATH" \
    AWS_ACCESS_KEY_ID=mock \
    AWS_SECRET_ACCESS_KEY=mock \
    MOCK_R2_ROOT="${case_dir}/r2" \
    MOCK_R2_FAIL_DELETE="$orphan" \
    CLOUDFLARE_R2_BUCKET_NAME=test-bucket \
    CLOUDFLARE_R2_PREFIX="$prefix" \
    CLOUDFLARE_R2_URL=https://r2.invalid \
    REPO_ROOT="$repo_root" \
    bash "${repo_root}/scripts/purge-orphan-dev-wheels.sh" --apply

  assert_file "${case_dir}/r2/${orphan}"
  if grep -Fq "$orphan" "${case_dir}/r2/index.html"; then
    fail "Failed orphan deletion left a stale index link"
  fi
}

test_publication_transaction() {
  local version="2.0.0rc3.dev20260803+451"
  local success="${work_dir}/transaction-success"
  local failure="${work_dir}/transaction-failure"
  local upload_failure="${work_dir}/transaction-upload-failure"
  local stale="${work_dir}/transaction-stale"
  local mismatch="${work_dir}/transaction-mismatch"
  local extra="${work_dir}/transaction-extra"
  local old="nautilus_trader-2.0.0rc3.dev20260802+400-cp313-cp313-manylinux_2_34_x86_64.whl"
  local output="${work_dir}/transaction-output"

  prepare_transaction "$success" "$version"
  run_transaction "$success" "$version"
  assert_absent "${success}/r2/${old}"
  assert_file "${success}/r2/nautilus_trader-2.0.0rc3-cp313-cp313-manylinux_2_34_x86_64.whl"
  assert_file "${success}/r2/nautilus_trader-2.0.0rc3.dev20260802-cp313-cp313-manylinux_2_34_x86_64.whl"
  assert_file "${success}/r2/nautilus_trader-2.0.0a20260802-cp313-cp313-manylinux_2_34_x86_64.whl"
  assert_file "${success}/r2/other-object.txt"
  cmp -s "${success}/generated-index.html" "${success}/r2/index.html" ||
    fail "Published index did not match generated index"
  grep -Fq "nautilus-trader==${version}" "${success}/uv.log" ||
    fail "Public-index install did not pin the publication version"

  prepare_transaction "$failure" "$version"
  run_expect_failure "$output" run_transaction "$failure" "$version" "$old"
  assert_file "${failure}/r2/${old}"
  if grep -Fq "$old" "${failure}/r2/index.html"; then
    fail "Failed deletion left a stale index link"
  fi
  cmp -s "${failure}/generated-index.html" "${failure}/r2/index.html" ||
    fail "Failed deletion changed the verified index"

  prepare_transaction "$upload_failure" "$version"
  cp "${upload_failure}/r2/index.html" "${upload_failure}/original-index.html"
  failed_upload="nautilus_trader-${version}-cp314-cp314-manylinux_2_34_x86_64.whl"
  run_expect_failure "$output" run_transaction "$upload_failure" "$version" '' "$failed_upload"
  cmp -s "${upload_failure}/original-index.html" "${upload_failure}/r2/index.html" ||
    fail "Partial upload changed the old index"
  assert_file "${upload_failure}/r2/nautilus_trader-${version}-cp312-cp312-manylinux_2_34_x86_64.whl"
  assert_file "${upload_failure}/r2/nautilus_trader-${version}-cp313-cp313-manylinux_2_34_x86_64.whl"
  assert_absent "${upload_failure}/r2/${failed_upload}"
  assert_file "${upload_failure}/r2/${old}"

  prepare_transaction "$stale" "$version"
  printf 'newer-development\n' > \
    "${stale}/r2/nautilus_trader-2.0.0rc3.dev20260803+452-cp313-cp313-manylinux_2_34_x86_64.whl"
  write_index "${stale}/r2"
  cp "${stale}/r2/index.html" "${stale}/original-index.html"
  run_transaction "$stale" "$version" > "$output" 2>&1
  assert_line "$output" \
    "More recent development wheels (20260803+452) already exist; skipping upload of ${version}"
  cmp -s "${stale}/original-index.html" "${stale}/r2/index.html" ||
    fail "Stale publication changed the index"
  assert_file "${stale}/r2/${old}"
  if [[ -s "${stale}/uv.log" ]]; then
    fail "Stale publication ran public-index verification"
  fi
  for python_tag in cp312 cp313 cp314; do
    assert_absent \
      "${stale}/r2/nautilus_trader-${version}-${python_tag}-${python_tag}-manylinux_2_34_x86_64.whl"
  done

  prepare_transaction "$mismatch" "$version"
  printf 'wrong-current-bytes\n' > \
    "${mismatch}/r2/nautilus_trader-${version}-cp312-cp312-manylinux_2_34_x86_64.whl"
  write_index "${mismatch}/r2"
  cp "${mismatch}/r2/index.html" "${mismatch}/original-index.html"
  run_expect_failure "$output" run_transaction "$mismatch" "$version"
  cmp -s "${mismatch}/original-index.html" "${mismatch}/r2/index.html" ||
    fail "Mismatched existing object changed the index"
  mismatched_bytes="$(cat \
    "${mismatch}/r2/nautilus_trader-${version}-cp312-cp312-manylinux_2_34_x86_64.whl")"
  if [[ "$mismatched_bytes" != "wrong-current-bytes" ]]; then
    fail "Mismatched existing object was overwritten"
  fi

  prepare_transaction "$extra" "$version"
  printf 'unexpected-current-target\n' > \
    "${extra}/r2/nautilus_trader-${version}-cp311-cp311-manylinux_2_34_x86_64.whl"
  write_index "${extra}/r2"
  cp "${extra}/r2/index.html" "${extra}/original-index.html"
  run_expect_failure "$output" run_transaction "$extra" "$version"
  cmp -s "${extra}/original-index.html" "${extra}/r2/index.html" ||
    fail "Unexpected current-version object changed the index"
  assert_absent "${extra}/r2/nautilus_trader-${version}-cp312-cp312-manylinux_2_34_x86_64.whl"
}

test_policy_and_version
test_security_gate
test_sha256_portability
test_artifact_matrix
test_stable_retention_plan
test_publication_transaction
test_orphan_purge_index_order

echo "Wheel publishing tests passed."
