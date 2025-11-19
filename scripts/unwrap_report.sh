#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT_DIR"

OUT=unwrap_report.txt

# Prefer ripgrep if available, otherwise fall back to grep -R
if command -v rg >/dev/null 2>&1; then
  rg -n --no-heading \
    --glob '!**/tests/**' \
    --glob '!**/benches/**' \
    --glob '!**/target/**' \
    --glob '!**/node_modules/**' \
    --glob '!**/.venv/**' \
    '(unwrap\(|expect\()' crates | sort -u | tee "$OUT"
else
  grep -RInE --exclude-dir=tests --exclude-dir=benches --exclude-dir=target \
    --exclude-dir=node_modules --exclude-dir=.venv \
    -E '(unwrap\(|expect\()' crates | sort -u | tee "$OUT"
fi

echo
echo "Report written to $OUT"
