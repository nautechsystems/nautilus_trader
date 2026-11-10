#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
REAL_SORT=$(command -v sort)
REAL_COMM=$(command -v comm)
CASE_ROOT=$(mktemp -d)
trap 'rm -rf "$CASE_ROOT"' EXIT

if ! command -v rg > /dev/null 2>&1; then
  echo 'ripgrep is required for comparison locale tests' >&2
  exit 1
fi

mkdir -p "$CASE_ROOT/bin" "$CASE_ROOT/copyright" \
  "$CASE_ROOT/conventions/crates/pyo3/src" "$CASE_ROOT/conventions/examples"
git -C "$CASE_ROOT/copyright" init --quiet
CURRENT_YEAR=$(date -u +%Y)
printf '# Copyright (C) 2015-%s\n' "$CURRENT_YEAR" > "$CASE_ROOT/copyright/B.py"
printf '# Copyright (C) 2015-%s\n' "$CURRENT_YEAR" > "$CASE_ROOT/copyright/a.py"
printf '# Missing header\n' > "$CASE_ROOT/copyright/space path.py"
git -C "$CASE_ROOT/copyright" add B.py a.py 'space path.py'
cp "$REPO_ROOT/crates/pyo3/src/lib.rs" "$CASE_ROOT/conventions/crates/pyo3/src/lib.rs"

cat > "$CASE_ROOT/bin/sort" << 'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'sort:%s\n' "${LC_ALL:-unset}" >> "$TEST_LOCALE_PROBE"
if [[ "${LC_ALL:-}" != C ]]; then
  echo 'sort did not receive the C locale' >&2
  exit 17
fi
exec "$TEST_REAL_SORT" "$@"
EOF

cat > "$CASE_ROOT/bin/comm" << 'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'comm:%s\n' "${LC_ALL:-unset}" >> "$TEST_LOCALE_PROBE"
if [[ "${LC_ALL:-}" != C ]]; then
  echo 'comm did not receive the C locale' >&2
  exit 18
fi
exec "$TEST_REAL_COMM" "$@"
EOF
chmod +x "$CASE_ROOT/bin/sort" "$CASE_ROOT/bin/comm"

printf 'sort:C\nsort:C\ncomm:C\nsort:C\nsort:C\ncomm:C\ncomm:C\n' \
  > "$CASE_ROOT/expected"

for caller_locale in all categories; do
  : > "$CASE_ROOT/probe"
  (
    if [[ "$caller_locale" == all ]]; then
      export LC_ALL=en_US.UTF-8
    else
      unset LC_ALL
      export LANG=en_US.UTF-8 LC_COLLATE=en_US.UTF-8
    fi

    export PATH="$CASE_ROOT/bin:$PATH"
    export TEST_LOCALE_PROBE="$CASE_ROOT/probe"
    export TEST_REAL_SORT="$REAL_SORT" TEST_REAL_COMM="$REAL_COMM"

    cd "$CASE_ROOT/copyright"
    "$BASH" "$SCRIPT_DIR/check_copyright_year.sh" > "$CASE_ROOT/copyright-output" 2>&1
    cd "$CASE_ROOT/conventions"
    "$BASH" "$SCRIPT_DIR/check_nautilus_conventions.sh" \
      > "$CASE_ROOT/conventions-output" 2>&1
  )

  cmp "$CASE_ROOT/expected" "$CASE_ROOT/probe"
  grep -Fq 'WARNING: space path.py: Missing copyright header' "$CASE_ROOT/copyright-output"
  grep -Fq 'All copyright years are current' "$CASE_ROOT/copyright-output"
  grep -Fq 'Python submodule registrations are unchanged' "$CASE_ROOT/conventions-output"
  grep -Fq 'All Nautilus conventions are valid' "$CASE_ROOT/conventions-output"
done

echo 'Comparison locale tests passed'
