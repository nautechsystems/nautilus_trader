#!/usr/bin/env bash
set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
script="$repo_root/scripts/ci/package-cli-artifact.bash"
case_root=$(mktemp -d)
trap 'rm -rf "$case_root"' EXIT
workspace="$case_root/workspace"

mkdir -p "$workspace/target/release" "$workspace/crates/cli"
printf 'binary\n' > "$workspace/target/release/nautilus"
printf 'license\n' > "$workspace/crates/cli/LICENSE"
printf 'readme\n' > "$workspace/crates/cli/README.md"

(
  cd "$workspace"
  bash "$script" x86_64-unknown-linux-gnu
)

archive="$workspace/dist/nautilus-x86_64-unknown-linux-gnu.tar.gz"
test -f "$archive"
tar -tzf "$archive" > "$case_root/archive-list"
grep -Fxq './nautilus' "$case_root/archive-list"
grep -Fxq './LICENSE' "$case_root/archive-list"
grep -Fxq './README.md' "$case_root/archive-list"
test ! -d "$workspace/stage-cli"

if (cd "$workspace" && bash "$script" '../invalid') > /dev/null 2>&1; then
  echo "Expected an invalid CLI target to fail" >&2
  exit 1
fi

echo "CLI artifact packaging tests passed"
