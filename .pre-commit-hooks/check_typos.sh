#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
args=(--force-exclude --threads 2)

for file in "$@"; do
  case "$file" in
    .pre-commit-config.yaml | .typos.toml | */.typos.toml | docs/developer_guide/coding_standards.md | .pre-commit-hooks/check_typos.sh)
      git ls-files | typos "${args[@]}" --file-list -
      exit 0
      ;;
  esac
done

exec typos "${args[@]}" -- "$@"
