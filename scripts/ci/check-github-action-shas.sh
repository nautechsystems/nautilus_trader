#!/usr/bin/env bash
# Verify that external GitHub Actions identify their source and pin the expected release tag:
# Expected format:
#   # https://github.com/owner/repo
#   uses: owner/repo/path@<sha> # <tag>

# This check collects every failure before exiting, so errexit would stop at handled command
# failures. Pipeline statuses are inspected explicitly so valid no-match results remain nonfatal.
set -uo pipefail

if [[ $# -eq 0 ]]; then
  echo "Usage: $0 <action-file>..." >&2
  exit 2
fi

for action_file in "$@"; do
  if [[ ! -f "$action_file" || ! -r "$action_file" ]]; then
    echo "Action file is not a readable regular file: $action_file" >&2
    exit 2
  fi
done

USES_LINES=""
ACTION_FAILURES=""
cleanup() {
  [[ -z "$USES_LINES" ]] || rm -f "$USES_LINES"
  [[ -z "$ACTION_FAILURES" ]] || rm -f "$ACTION_FAILURES"
}
trap cleanup EXIT

if ! USES_LINES=$(mktemp "${TMPDIR:-/tmp}/nautilus-action-uses.XXXXXX"); then
  echo "Could not create the temporary action reference file" >&2
  exit 2
fi
if ! ACTION_FAILURES=$(mktemp "${TMPDIR:-/tmp}/nautilus-action-failures.XXXXXX"); then
  echo "Could not create the temporary action failure file" >&2
  exit 2
fi

FAILED=0

if ! awk '
  FNR == 1 {
    previous = ""
  }

  /^[[:space:]]*-?[[:space:]]*uses:[[:space:]]*/ {
    reference = $0
    sub(/^[[:space:]]*-?[[:space:]]*uses:[[:space:]]*/, "", reference)
    sub(/[[:space:]#].*$/, "", reference)

    if (reference !~ /^\.\// && reference !~ /^docker:\/\//) {
      action = reference
      sub(/@.*/, "", action)
      split(action, parts, "/")
      expected = "# https://github.com/" parts[1] "/" parts[2]
      actual = previous
      sub(/^[[:space:]]*/, "", actual)
      sub(/[[:space:]]*$/, "", actual)

      if (actual != expected) {
        printf "%s:%d: FAILED (expected source comment %s immediately above): %s\n", \
          FILENAME, FNR, expected, reference
      }

      pin = reference
      sub(/^.*@/, "", pin)
      if (length(pin) != 40 || pin ~ /[^0-9a-f]/) {
        printf "%s:%d: FAILED (expected full 40-character commit SHA): %s\n", \
          FILENAME, FNR, reference
      }
    }
  }

  {
    previous = $0
  }
' "$@" > "$ACTION_FAILURES"; then
  echo "Could not inspect the GitHub Action files" >&2
  exit 2
fi

if [[ -s "$ACTION_FAILURES" ]]; then
  cat "$ACTION_FAILURES"
  FAILED=1
fi

grep -h '^[[:space:]]*-*[[:space:]]*uses:[[:space:]]*' "$@" |
  grep '@[0-9a-f]\{40\}' |
  sed -e 's/^[[:space:]]*-\{0,1\}[[:space:]]*uses:[[:space:]]*//' |
  sort -u > "$USES_LINES"
pipeline_status=("${PIPESTATUS[@]}")
if ((pipeline_status[0] > 1 || pipeline_status[1] > 1 || \
  pipeline_status[2] != 0 || pipeline_status[3] != 0)); then
  echo "Could not collect GitHub Action references" >&2
  exit 2
fi

if [[ ! -s "$USES_LINES" ]]; then
  echo "No GitHub Action SHAs found."
  exit "$FAILED"
fi

while IFS= read -r line; do
  REPO_WITH_PATH=$(printf '%s\n' "$line" | cut -d'@' -f1)
  REPO=$(printf '%s\n' "$REPO_WITH_PATH" | cut -d'/' -f1,2)
  EXPECTED_SHA=$(printf '%s\n' "$line" | cut -d'@' -f2 | cut -d' ' -f1)
  if ! printf '%s\n' "$line" | grep -q '#[[:space:]]*[^[:space:]]'; then
    echo "FAILED (missing '# <tag>' comment): $line"
    FAILED=1
    continue
  fi

  TAG=$(printf '%s\n' "$line" | cut -d'#' -f2 | tr -d ' ')

  printf 'Checking %s (%s): ' "$REPO_WITH_PATH" "$TAG"

  URL="https://github.com/$REPO.git"
  REMOTE_OUT=$(git ls-remote "$URL" "refs/tags/$TAG" "refs/tags/$TAG^{}" 2> /dev/null)

  if [[ -z "$REMOTE_OUT" ]]; then
    echo "FAILED (tag not found or repository inaccessible)"
    FAILED=1
    continue
  fi

  COMMIT_SHA=$(printf '%s\n' "$REMOTE_OUT" | grep '\^{}' | awk '{print $1}')
  if [[ -z "$COMMIT_SHA" ]]; then
    COMMIT_SHA=$(printf '%s\n' "$REMOTE_OUT" | awk '{print $1}')
  fi

  if [[ "$COMMIT_SHA" == "$EXPECTED_SHA" ]]; then
    echo "OK ($EXPECTED_SHA)"
  else
    echo "MISMATCH (expected: $EXPECTED_SHA, got: $COMMIT_SHA)"
    FAILED=1
  fi
done < "$USES_LINES"

if [[ "$FAILED" -eq 1 ]]; then
  exit 1
fi
