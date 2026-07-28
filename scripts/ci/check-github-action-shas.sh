#!/bin/bash
# A script to verify that GitHub Action SHAs match their expected release tags.
# It expects the format: uses: owner/repo/path@<sha> # <tag>

if [ "$#" -eq 0 ]; then
  echo "Usage: $0 <action-file>..." >&2
  exit 2
fi

USES_LINES="$(mktemp)"
trap 'rm -f "$USES_LINES"' EXIT

grep -h '^[[:space:]]*-*[[:space:]]*uses:[[:space:]]*' "$@" |
  grep '@[0-9a-f]\{40\}' |
  sed -e 's/^[[:space:]]*-\{0,1\}[[:space:]]*uses:[[:space:]]*//' |
  sort -u > "$USES_LINES"

if [ ! -s "$USES_LINES" ]; then
  echo "No GitHub Action SHAs found."
  exit 0
fi

FAILED=0

while IFS= read -r line; do
  REPO_WITH_PATH=$(echo "$line" | cut -d'@' -f1)
  REPO=$(echo "$REPO_WITH_PATH" | cut -d'/' -f1,2)
  EXPECTED_SHA=$(echo "$line" | cut -d'@' -f2 | cut -d' ' -f1)
  if ! echo "$line" | grep -q '#[[:space:]]*[^[:space:]]'; then
    echo "FAILED (missing '# <tag>' comment): $line"
    FAILED=1
    continue
  fi

  TAG=$(echo "$line" | cut -d'#' -f2 | tr -d ' ')

  echo -n "Checking $REPO_WITH_PATH ($TAG)... "

  URL="https://github.com/$REPO.git"
  REMOTE_OUT=$(git ls-remote "$URL" "refs/tags/$TAG" "refs/tags/$TAG^{}" 2> /dev/null)

  if [ -z "$REMOTE_OUT" ]; then
    echo "FAILED (tag not found or repo inaccessible)"
    FAILED=1
    continue
  fi

  COMMIT_SHA=$(echo "$REMOTE_OUT" | grep '\^{}' | awk '{print $1}')
  if [ -z "$COMMIT_SHA" ]; then
    COMMIT_SHA=$(echo "$REMOTE_OUT" | awk '{print $1}')
  fi

  if [ "$COMMIT_SHA" = "$EXPECTED_SHA" ]; then
    echo "OK ($EXPECTED_SHA)"
  else
    echo "MISMATCH (Expected: $EXPECTED_SHA, Got: $COMMIT_SHA)"
    FAILED=1
  fi
done < "$USES_LINES"

if [ "$FAILED" -eq 1 ]; then
  exit 1
fi
