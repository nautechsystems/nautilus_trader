#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)
CASE_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/nautilus-typos.XXXXXX")
trap 'rm -rf "$CASE_ROOT"' EXIT
mkdir -p "$CASE_ROOT/bin" "$CASE_ROOT/repo/.pre-commit-hooks"
cp "$REPO_ROOT/.pre-commit-hooks/check_typos.sh" "$CASE_ROOT/repo/.pre-commit-hooks/"

cat > "$CASE_ROOT/bin/typos" << 'SH'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$@" > "$CASE_ROOT/args"
if [[ " $* " == *" --file-list - "* ]]; then
  cat > "$CASE_ROOT/files"
fi
exit "${TYPOS_STATUS:-0}"
SH

cat > "$CASE_ROOT/bin/git" << 'SH'
#!/usr/bin/env bash
set -euo pipefail
[[ "$PWD" == "$CASE_ROOT/repo" ]]
[[ "$*" == ls-files ]]
printf '%s\n' README.md 'docs/with spaces.md'
exit "${GIT_STATUS:-0}"
SH

chmod +x "$CASE_ROOT/bin/typos" "$CASE_ROOT/bin/git"
export CASE_ROOT
export PATH="$CASE_ROOT/bin:$PATH"
cd "$CASE_ROOT"
check="$CASE_ROOT/repo/.pre-commit-hooks/check_typos.sh"

bash "$check" 'docs/changed file.md' crates/core/src/lib.rs
printf '%s\n' --force-exclude --threads 2 -- 'docs/changed file.md' crates/core/src/lib.rs > expected
diff -u expected args
[[ ! -e files ]]

for policy in .pre-commit-config.yaml .typos.toml nested/.typos.toml \
  docs/developer_guide/coding_standards.md .pre-commit-hooks/check_typos.sh; do
  bash "$check" 'docs/changed file.md' "$policy"
  printf '%s\n' --force-exclude --threads 2 --file-list - > expected
  diff -u expected args
  printf '%s\n' README.md 'docs/with spaces.md' > expected
  diff -u expected files
done

for path in README.md .typos.toml; do
  status=0
  TYPOS_STATUS=7 bash "$check" "$path" || status=$?
  [[ "$status" == 7 ]]
done

status=0
GIT_STATUS=8 bash "$check" .typos.toml || status=$?
[[ "$status" == 8 ]]

echo "Typos hook tests passed"
