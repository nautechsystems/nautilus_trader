#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STABLE_VERSION_PATTERN='^[0-9]+\.[0-9]+\.[0-9]+$'
VERSION_PATTERN='[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?'
VERSION_OUTPUT_PATTERN="(^|[^0-9A-Za-z.])(${VERSION_PATTERN})([^0-9A-Za-z.+-]|$)"

security_version() {
  local tool=$1 reader=$2 version

  version=$(bash "$SCRIPT_DIR/$reader" "$tool")
  if ! [[ "$version" =~ $STABLE_VERSION_PATTERN ]]; then
    printf 'Error: %s version pin must be a stable X.Y.Z release: %s\n' \
      "$tool" "$version" >&2
    exit 1
  fi
  printf '%s' "$version"
}

CARGO_AUDIT_VERSION=$(security_version cargo-audit cargo-tool-version.sh)
CARGO_DENY_VERSION=$(security_version cargo-deny cargo-tool-version.sh)
CARGO_VET_VERSION=$(security_version cargo-vet cargo-tool-version.sh)
OSV_SCANNER_VERSION=$(security_version osv-scanner tool-version.sh)

if ! command -v cargo > /dev/null 2>&1; then
  echo "Error: cargo is required to install the Cargo supply-chain tools" >&2
  exit 1
fi

installed_version() {
  local subcommand=$1 output=""

  output=$(cargo "$subcommand" --version 2> /dev/null) || true
  reported_version "$output"
}

reported_version() {
  local output=$1

  if [[ "$output" =~ $VERSION_OUTPUT_PATTERN ]]; then
    printf '%s' "${BASH_REMATCH[2]}"
  fi
}

install_cargo_tool() {
  local package=$1 subcommand=$2 required=$3 current

  current=$(installed_version "$subcommand")
  if [[ "$current" == "$required" ]]; then
    printf '%s %s is already installed\n' "$package" "$required"
    return
  fi

  if [[ -n "$current" ]]; then
    printf 'Updating %s from %s to %s\n' "$package" "$current" "$required"
  else
    printf 'Installing %s %s\n' "$package" "$required"
  fi
  cargo install "$package" --version "$required" --locked --force

  current=$(installed_version "$subcommand")
  if [[ "$current" != "$required" ]]; then
    printf 'Error: %s version mismatch after install: expected %s, found %s\n' \
      "$package" "$required" "${current:-not installed}" >&2
    exit 1
  fi
}

install_cargo_tool cargo-audit audit "$CARGO_AUDIT_VERSION"
install_cargo_tool cargo-deny deny "$CARGO_DENY_VERSION"
install_cargo_tool cargo-vet vet "$CARGO_VET_VERSION"
bash "$SCRIPT_DIR/install-osv-scanner.sh"

if ! command -v osv-scanner > /dev/null 2>&1; then
  echo "Error: osv-scanner was installed outside PATH" >&2
  exit 1
fi
osv_output=$(osv-scanner --version 2>&1) || true
installed_osv=$(reported_version "$osv_output")
if [[ "$installed_osv" != "$OSV_SCANNER_VERSION" ]]; then
  printf 'Error: osv-scanner version mismatch after install: expected %s, found %s\n' \
    "$OSV_SCANNER_VERSION" "${installed_osv:-not installed}" >&2
  exit 1
fi

echo "All shared supply-chain tools are installed at their central versions"
