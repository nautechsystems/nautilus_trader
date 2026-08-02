#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
CHECK_SCRIPT="${SCRIPT_DIR}/check-miri-toolchain.bash"

bash "$CHECK_SCRIPT"

test_root="$(mktemp -d)"
trap 'rm -rf "$test_root"' EXIT

mkdir -p "${test_root}/scripts/ci"
cp "$CHECK_SCRIPT" "${test_root}/scripts/ci/check-miri-toolchain.bash"
cp "${REPO_ROOT}/scripts/tool-version.sh" "${test_root}/scripts/tool-version.sh"

write_versions() {
  local miri_rustc_version="$1"
  local workspace_rust_version="$2"

  printf '[miri]\nversion = "nightly-test"\nrustc-version = "%s"\n' \
    "$miri_rustc_version" > "${test_root}/tools.toml"
  printf '[workspace.package]\nrust-version = "%s"\n' \
    "$workspace_rust_version" > "${test_root}/Cargo.toml"
}

expect_success() {
  local miri_rustc_version="$1"
  local workspace_rust_version="$2"

  write_versions "$miri_rustc_version" "$workspace_rust_version"
  if ! bash "${test_root}/scripts/ci/check-miri-toolchain.bash" > "${test_root}/output" 2>&1; then
    echo "Expected Miri rustc ${miri_rustc_version} to support workspace Rust ${workspace_rust_version}" >&2
    cat "${test_root}/output" >&2
    exit 1
  fi
}

expect_failure() {
  local miri_rustc_version="$1"
  local workspace_rust_version="$2"

  write_versions "$miri_rustc_version" "$workspace_rust_version"
  if bash "${test_root}/scripts/ci/check-miri-toolchain.bash" > "${test_root}/output" 2>&1; then
    echo "Expected Miri rustc ${miri_rustc_version} to reject workspace Rust ${workspace_rust_version}" >&2
    exit 1
  fi

  grep -Fq "below workspace rust-version" "${test_root}/output"
}

expect_success "1.99.0" "1.99.0"
expect_success "1.99.0" "1.97.1"
expect_success "1.99.0" "1.97"
expect_success "1.99.0" "1"
expect_failure "1.99.0" "1.99.1"
expect_failure "1.99.0" "1.100.0"
expect_failure "1.99.0" "2"

fake_bin="${test_root}/bin"
mkdir -p "$fake_bin"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'echo "rustup warning" >&2' \
  'echo "rustc 1.99.0-nightly (test 2026-07-14)"' \
  > "${fake_bin}/rustc"
chmod +x "${fake_bin}/rustc"

write_versions "1.99.0" "1.97.1"
PATH="${fake_bin}:${PATH}" bash "${test_root}/scripts/ci/check-miri-toolchain.bash" \
  --verify-installed > "${test_root}/output" 2>&1

write_versions "1.98.0" "1.97.1"
if PATH="${fake_bin}:${PATH}" bash "${test_root}/scripts/ci/check-miri-toolchain.bash" \
  --verify-installed > "${test_root}/output" 2>&1; then
  echo "Expected installed rustc mismatch to fail" >&2
  exit 1
fi
grep -Fq "provides rustc 1.99.0, expected 1.98.0" "${test_root}/output"

echo "Miri toolchain check tests passed"
