#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
HOOK="$SCRIPT_DIR/check_copyright_year.sh"
CASE_ROOT=$(mktemp -d)
trap 'rm -rf "$CASE_ROOT"' EXIT

mkdir -p "$CASE_ROOT/repo" "$CASE_ROOT/temp dir" "$CASE_ROOT/bin"
git -C "$CASE_ROOT/repo" init --quiet
CURRENT_YEAR=$(date -u +%Y)

run_hook() {
  (cd "$CASE_ROOT/repo" && TMPDIR="$CASE_ROOT/temp dir" bash "$HOOK") \
    > "$CASE_ROOT/output" 2>&1
}

check_cleanup() {
  if [[ -n "$(ls -A "$CASE_ROOT/temp dir")" ]]; then
    echo "Copyright hook left temporary files behind"
    exit 1
  fi
}

printf '# Copyright (C) 2015-%s\n' "$CURRENT_YEAR" > "$CASE_ROOT/repo/sample.py"
git -C "$CASE_ROOT/repo" add sample.py
run_hook
grep -Fq 'All copyright years are current' "$CASE_ROOT/output"
check_cleanup

printf '# Copyright (C) 2015-%s\n' "$((CURRENT_YEAR - 1))" > "$CASE_ROOT/repo/sample.py"
if run_hook; then
  echo "Copyright hook accepted an expired year"
  exit 1
fi
grep -Fq 'ERROR: sample.py: Copyright year is' "$CASE_ROOT/output"
check_cleanup

printf '# No copyright header\n' > "$CASE_ROOT/repo/sample.py"
run_hook
grep -Fq 'WARNING: sample.py: Missing copyright header' "$CASE_ROOT/output"
check_cleanup

if (cd "$CASE_ROOT/repo" && TMPDIR="$CASE_ROOT/absent" bash "$HOOK") \
  > "$CASE_ROOT/output" 2>&1; then
  echo "Copyright hook accepted an unavailable temporary directory"
  exit 1
fi
check_cleanup

cat > "$CASE_ROOT/bin/sort" << 'EOF'
#!/usr/bin/env bash
echo 'Injected sort failure' >&2
exit 17
EOF
chmod +x "$CASE_ROOT/bin/sort"
if PATH="$CASE_ROOT/bin:$PATH" run_hook; then
  echo "Copyright hook ignored a failed inventory sort"
  exit 1
fi
grep -Fq 'Injected sort failure' "$CASE_ROOT/output"
check_cleanup

echo 'Copyright hook temporary-directory tests passed'
