#!/usr/bin/env bash

set -euo pipefail

if ! command -v rg &> /dev/null; then
  echo "ERROR: ripgrep is required for Python formatting hook tests" >&2
  echo "       install from: https://github.com/BurntSushi/ripgrep#installation" >&2
  exit 1
fi

REPO_ROOT=$(git rev-parse --show-toplevel)
HOOK="$REPO_ROOT/.pre-commit-hooks/check_formatting_py.sh"

CASE_ROOT=$(mktemp -d)
trap 'rm -rf "$CASE_ROOT"' EXIT

write_py() {
  local path="$1"
  shift

  mkdir -p "$(dirname "$path")"
  printf '%s\n' "$@" > "$path"
}

create_case() {
  local case_dir="$1"

  mkdir -p "$case_dir"/{docs,examples,python}
}

run_hook() {
  local case_dir="$1"

  (cd "$case_dir" && bash "$HOOK") > "$case_dir/output.txt" 2>&1
}

expect_failure() {
  local case_dir="$1"
  local pattern="$2"

  if run_hook "$case_dir"; then
    echo "Expected Python formatting hook to fail in $case_dir"
    cat "$case_dir/output.txt"
    exit 1
  fi

  rg -q "$pattern" "$case_dir/output.txt"
}

expect_success() {
  local case_dir="$1"

  if ! run_hook "$case_dir"; then
    echo "Expected Python formatting hook to pass in $case_dir"
    cat "$case_dir/output.txt"
    exit 1
  fi
}

for control_flow in if match for while; do
  control_flow_case="$CASE_ROOT/reject-missing-blank-$control_flow"
  create_case "$control_flow_case"

  case "$control_flow" in
    if)
      write_py "$control_flow_case/python/example.py" \
        'def check(flag):' \
        '    prepare()' \
        '    if flag:' \
        '        run()'
      ;;
    match)
      write_py "$control_flow_case/python/example.py" \
        'def check(state):' \
        '    prepare()' \
        '    match state:' \
        '        case "ready":' \
        '            run()'
      ;;
    for)
      write_py "$control_flow_case/python/example.py" \
        'def check(items):' \
        '    prepare()' \
        '    for item in items:' \
        '        consume(item)'
      ;;
    while)
      write_py "$control_flow_case/python/example.py" \
        'def check(active):' \
        '    prepare()' \
        '    while active:' \
        '        run()'
      ;;
  esac

  expect_failure "$control_flow_case" "Missing blank line above .${control_flow}."
done

valid_case="$CASE_ROOT/allow-valid-control-flow"
create_case "$valid_case"
write_py "$valid_case/python/example.py" \
  'def choose(flag):' \
  '    if flag:' \
  '        return "ready"' \
  '' \
  '' \
  'def select(state):' \
  '    """Select a state."""' \
  '    match state:' \
  '        case "ready":' \
  '            return True' \
  '' \
  '' \
  'def visit(items):' \
  '    for item in items:' \
  '        consume(item)' \
  '' \
  '' \
  'def poll(active):' \
  '    while active:' \
  '        run()' \
  '' \
  '' \
  'def filter_ready(items):' \
  '    return [' \
  '        item' \
  '        for item in items' \
  '        if item.ready' \
  '    ]'
expect_success "$valid_case"

echo "Python formatting hook tests passed"
