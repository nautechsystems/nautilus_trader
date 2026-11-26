#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

for catalog in \
  "${REPO_ROOT}/.nautilus-engineering/tools.toml" \
  "${REPO_ROOT}/tools.toml"; do
  tool_count=0
  while IFS= read -r tool; do
    version=$(bash "${REPO_ROOT}/scripts/tool-version.sh" "$tool")
    if [[ -z "$version" ]]; then
      printf 'Tool version was empty for %s\n' "$tool" >&2
      exit 1
    fi
    tool_count=$((tool_count + 1))
  done < <(awk '
    /^\[[a-z0-9][a-z0-9_-]*\]$/ {
      section=$0
      gsub(/^\[|\]$/, "", section)
      print section
    }
  ' "$catalog")

  if ((tool_count == 0)); then
    printf 'No tool pins were found in %s\n' "$catalog" >&2
    exit 1
  fi
done

cargo_tool_count=0
while IFS= read -r tool; do
  version=$(bash "${REPO_ROOT}/scripts/cargo-tool-version.sh" "$tool")
  if [[ -z "$version" ]]; then
    printf 'Cargo tool version was empty for %s\n' "$tool" >&2
    exit 1
  fi
  cargo_tool_count=$((cargo_tool_count + 1))
done < <(awk '
  /^\[workspace\.metadata\.tools\]/ { in_section=1; next }
  /^\[/ { in_section=0 }
  in_section && /^[a-z0-9][a-z0-9_-]*[[:space:]]*=/ { print $1 }
' "${REPO_ROOT}/Cargo.toml")

if ((cargo_tool_count == 0)); then
  echo "No local Cargo tool pins were found" >&2
  exit 1
fi

for tool in cargo-audit cargo-deny cargo-edit cargo-llvm-cov cargo-nextest cargo-vet; do
  if [[ -z $(bash "${REPO_ROOT}/scripts/cargo-tool-version.sh" "$tool") ]]; then
    printf 'Shared Cargo tool version was empty for %s\n' "$tool" >&2
    exit 1
  fi
done

uv_version=$(bash "${REPO_ROOT}/scripts/uv-version.sh")
if [[ "$uv_version" != $(bash "${REPO_ROOT}/scripts/tool-version.sh" uv) ]]; then
  echo "uv-version.sh did not match the shared uv pin" >&2
  exit 1
fi

rust_toolchain=$(bash "${REPO_ROOT}/scripts/rust-toolchain.sh")
if [[ -z "$rust_toolchain" ]]; then
  echo "Rust toolchain version was empty" >&2
  exit 1
fi

case_root=$(mktemp -d)
trap 'rm -rf "$case_root"' EXIT
mkdir -p "$case_root/scripts" "$case_root/python"
cp "${REPO_ROOT}/scripts/package-version.sh" "$case_root/scripts/"
printf '[project]\nversion = "1.2.4"\n' > "$case_root/python/pyproject.toml"
if [[ "$(bash "$case_root/scripts/package-version.sh")" != 1.2.4 ]]; then
  echo "Package version lookup must work without the Rust sources" >&2
  exit 1
fi

mkdir -p "$case_root/crates/core"
printf '%s\n' 'const NAUTILUS_VERSION: &str = "1.2.3";' > "$case_root/crates/core/build.rs"

for version in 1.2.3 1.2.3a20261126 1.2.3.dev20261126+100 1.2.3rc1.dev20261126; do
  printf '[project]\nversion = "%s"\n' "$version" > "$case_root/python/pyproject.toml"
  output=$(bash "$case_root/scripts/package-version.sh" --check)
  if [[ "$output" != "$version" ]]; then
    printf 'Expected package version %s, received %s\n' "$version" "$output" >&2
    exit 1
  fi
done

for version in 1.2.4 1.2.30 1.2.3.4; do
  printf '[project]\nversion = "%s"\n' "$version" > "$case_root/python/pyproject.toml"
  if output=$(bash "$case_root/scripts/package-version.sh" --check 2>&1) ||
    [[ "$output" != *"Python package version $version does not match Rust version 1.2.3"* ]]; then
    printf 'Expected package version check to reject %s, received %s\n' "$version" "$output" >&2
    exit 1
  fi
done

printf '%s\n' 'const NAUTILUS_VERSION: &str = "1.2.3rc1";' > "$case_root/crates/core/build.rs"
printf '[project]\nversion = "1.2.3rc1.dev20261126"\n' > "$case_root/python/pyproject.toml"
[[ "$(bash "$case_root/scripts/package-version.sh" --check)" == 1.2.3rc1.dev20261126 ]]
printf '[project]\nversion = "1.2.3rc10"\n' > "$case_root/python/pyproject.toml"
if output=$(bash "$case_root/scripts/package-version.sh" --check 2>&1) ||
  [[ "$output" != *"Python package version 1.2.3rc10 does not match Rust version 1.2.3rc1"* ]]; then
  printf 'Expected package version check to reject a different release candidate, received %s\n' "$output" >&2
  exit 1
fi

core_fixture="$case_root/crates/core"
cp "${REPO_ROOT}/crates/core/build.rs" "${REPO_ROOT}/Cargo.lock" "$core_fixture/"
mkdir -p "$core_fixture/src"
printf 'pub const VERSION: &str = env!("NAUTILUS_VERSION");\n' > "$core_fixture/src/lib.rs"
python3 -B - "$REPO_ROOT" "$core_fixture/Cargo.toml" << 'PY'
import sys
import tomllib
from pathlib import Path

root, fixture = map(Path, sys.argv[1:])
core = tomllib.loads((root / "crates/core/Cargo.toml").read_text(encoding="utf-8"))
workspace = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
toml_dep = workspace["workspace"]["dependencies"]["toml"]
assert "dep:toml" in core["features"]["extension-module"]
assert core["build-dependencies"]["toml"]["optional"]
features = ", ".join(f'"{feature}"' for feature in toml_dep["features"])
fixture.write_text(
    '[package]\nname = "version-check-fixture"\nversion = "0.0.0"\nedition = "2024"\n'
    '[features]\nextension-module = ["dep:toml"]\n'
    f'[build-dependencies.toml]\nversion = "{toml_dep["version"]}"\n'
    f'optional = true\ndefault-features = false\nfeatures = [{features}]\n',
    encoding="utf-8",
)
PY

base_version=$(awk -F'"' '/^const NAUTILUS_VERSION: &str = / {print $2}' "$core_fixture/build.rs")
mismatched_version="${base_version}.1"
(
  cd "$core_fixture"
  export CARGO_TARGET_DIR="$case_root/target"
  cargo_check=(cargo "+$rust_toolchain" check --jobs 2 --color never)

  printf '[project]\nversion = "%s.dev20261126"\n' "$base_version" > "$case_root/python/pyproject.toml"
  "${cargo_check[@]}" --quiet --features extension-module
  "${cargo_check[@]}" --quiet

  printf '[project]\nversion = "%s"\n' "$mismatched_version" > "$case_root/python/pyproject.toml"
  if output=$("${cargo_check[@]}" --quiet --features extension-module 2>&1) ||
    [[ "$output" != *"Python package version $mismatched_version does not match Rust version $base_version"* ]]; then
    printf 'Expected the Python extension build to reject a changed base version, received %s\n' "$output" >&2
    exit 1
  fi

  output=$("${cargo_check[@]}" --verbose 2>&1)
  if [[ "$output" != *"Fresh version-check-fixture"* ]]; then
    printf 'Expected a Python manifest change to leave a Rust-only build fresh, received %s\n' "$output" >&2
    exit 1
  fi

  printf 'Metadata-Version: 2.4\n' > "$case_root/PKG-INFO"
  mv "$case_root/python/pyproject.toml" "$case_root/pyproject.toml"
  if output=$("${cargo_check[@]}" --quiet --features extension-module 2>&1) ||
    [[ "$output" != *"Python package version $mismatched_version does not match Rust version $base_version"* ]]; then
    printf 'Expected a source archive build to reject a changed base version, received %s\n' "$output" >&2
    exit 1
  fi
  printf '[project]\nversion = "%s"\n' "$base_version" > "$case_root/pyproject.toml"
  "${cargo_check[@]}" --quiet --features extension-module
)

echo "Tool version consumer tests passed"
