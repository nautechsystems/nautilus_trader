#!/usr/bin/env bash
# Build the source distribution and export its release asset path
set -euo pipefail

dist_dir="${1:-./dist}"

if ! command -v uv > /dev/null; then
  echo "::error::uv not found."
  exit 1
fi
if ! command -v git > /dev/null; then
  echo "::error::git not found."
  exit 1
fi
if ! command -v tar > /dev/null; then
  echo "::error::tar not found."
  exit 1
fi

mkdir -p "$dist_dir"
output_dist_dir="$(cd "$dist_dir" && pwd -P)"
build_root="$(mktemp -d)"
target_entries="$(mktemp)"
trap 'rm -rf "$build_root" "$target_entries"' EXIT

# Build from tracked files only. poetry-core explicit includes override .gitignore,
# so a direct workspace build can package local Rust target directories if present.
git archive --format=tar HEAD | tar -xf - -C "$build_root"
(
  cd "$build_root"
  uv build --sdist --out-dir "$output_dist_dir"
)

asset_path="$(
  find "$dist_dir" -maxdepth 1 -name '*.tar.gz' -type f -printf '%T@ %p\n' |
    sort -nr |
    head -n 1 |
    cut -d ' ' -f2-
)"

if [[ -z "$asset_path" ]]; then
  echo "::error::No .tar.gz files found in $dist_dir"
  exit 1
fi

if tar -tzf "$asset_path" | grep -E '/target(-v2)?/' > "$target_entries"; then
  echo "::error::sdist contains Rust target artifacts:"
  head -n 20 "$target_entries"
  exit 1
fi

echo "Built sdist: $asset_path"

if [[ -n "${GITHUB_ENV:-}" ]]; then
  {
    printf 'ASSET_PATH=%s\n' "$asset_path"
    printf 'ASSET_NAME=%s\n' "$(basename "$asset_path")"
  } >> "$GITHUB_ENV"
fi
