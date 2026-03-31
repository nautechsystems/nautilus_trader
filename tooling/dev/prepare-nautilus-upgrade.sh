#!/usr/bin/env bash

set -euo pipefail

REPO_ROOT="${REPO_ROOT_OVERRIDE:-$(cd "$(dirname "$0")/../.." && pwd)}"
UPSTREAM_REMOTE="${UPSTREAM_REMOTE:-upstream}"
UPSTREAM_URL="${UPSTREAM_URL:-https://github.com/nautechsystems/nautilus_trader.git}"
BASE_REMOTE="${BASE_REMOTE:-origin}"
BASE_BRANCH="${BASE_BRANCH:-main}"
UPGRADE_DATE="${UPGRADE_DATE:-$(date -u +%Y%m%d)}"

cd "$REPO_ROOT"

if ! git rev-parse --show-toplevel >/dev/null 2>&1; then
  echo "error: $REPO_ROOT is not a git repository" >&2
  exit 1
fi

if [[ -n "$(git status --porcelain --untracked-files=all)" ]]; then
  echo "error: working tree is not clean" >&2
  exit 1
fi

if ! git remote get-url "$UPSTREAM_REMOTE" >/dev/null 2>&1; then
  echo "Adding $UPSTREAM_REMOTE remote -> $UPSTREAM_URL"
  git remote add "$UPSTREAM_REMOTE" "$UPSTREAM_URL"
fi

git fetch "$UPSTREAM_REMOTE" --tags --prune
git fetch "$BASE_REMOTE" "$BASE_BRANCH" --prune

if [[ -n "${TARGET_TAG:-}" ]]; then
  target_tag="$TARGET_TAG"
else
  target_tag="$(
    git ls-remote --tags --refs "$UPSTREAM_REMOTE" 'v*' \
      | awk -F/ '{print $3}' \
      | sort -V \
      | tail -n 1
  )"
fi

if [[ -z "$target_tag" ]]; then
  echo "error: unable to determine target tag" >&2
  exit 1
fi

if ! git ls-remote --exit-code "$UPSTREAM_REMOTE" "refs/tags/$target_tag" >/dev/null 2>&1; then
  echo "error: tag $target_tag not found on $UPSTREAM_REMOTE" >&2
  exit 1
fi

if ! git show-ref --verify --quiet "refs/remotes/$BASE_REMOTE/$BASE_BRANCH"; then
  echo "error: base branch $BASE_REMOTE/$BASE_BRANCH does not exist locally after fetch" >&2
  exit 1
fi

upstream_branch="upstream-sync/$target_tag"
upgrade_branch="upgrade/nautilus-$UPGRADE_DATE-$target_tag"
evidence_path="${EVIDENCE_PATH:-docs/reviews/${UPGRADE_DATE}-nautilus-upstream-upgrade-${target_tag}.md}"
starting_branch="$(git branch --show-current)"

git checkout -B "$upstream_branch" "$target_tag"

if git show-ref --verify --quiet "refs/heads/$upgrade_branch"; then
  echo "error: upgrade branch $upgrade_branch already exists" >&2
  git checkout "$starting_branch" >/dev/null 2>&1 || true
  exit 1
fi

git checkout -b "$upgrade_branch" "$BASE_REMOTE/$BASE_BRANCH"

if git merge --no-ff --no-edit "$upstream_branch"; then
  :
else
  echo "merge conflict while merging $upstream_branch into $upgrade_branch" >&2
  echo "Resolve conflicts, then continue review on $upgrade_branch" >&2
  exit 2
fi

if [[ -n "${CHERRY_PICK_COMMITS:-}" ]]; then
  read -r -a commits <<< "$CHERRY_PICK_COMMITS"
  for commit in "${commits[@]}"; do
    if [[ ! "$commit" =~ ^[0-9a-fA-F]{7,40}$ ]]; then
      echo "error: invalid cherry-pick commit '$commit'" >&2
      exit 1
    fi
    if ! git rev-parse --verify --quiet "${commit}^{commit}" >/dev/null; then
      echo "error: cherry-pick commit '$commit' was not found locally" >&2
      exit 1
    fi
  done

  for commit in "${commits[@]}"; do
    git cherry-pick "$commit"
  done
fi

cat <<EOF
Prepared Nautilus upgrade branch
repo_root=$REPO_ROOT
target_tag=$target_tag
upstream_branch=$upstream_branch
upgrade_branch=$upgrade_branch
evidence_path=$evidence_path
EOF
