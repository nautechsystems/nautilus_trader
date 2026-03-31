#!/usr/bin/env bash

set -euo pipefail

SOURCE_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SCRIPT_PATH="$SOURCE_ROOT/tooling/dev/prepare-nautilus-upgrade.sh"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

upstream_seed="$tmpdir/upstream-seed"
upstream_bare="$tmpdir/upstream.git"
fork_repo="$tmpdir/fork"

mkdir -p "$upstream_seed"
git init "$upstream_seed" >/dev/null
git -C "$upstream_seed" config user.name "Test User"
git -C "$upstream_seed" config user.email "test@example.com"
printf 'upstream\n' >"$upstream_seed/README.md"
git -C "$upstream_seed" add README.md
git -C "$upstream_seed" commit -m "upstream init" >/dev/null
git -C "$upstream_seed" tag v1.224.0
git init --bare "$upstream_bare" >/dev/null
git -C "$upstream_seed" remote add origin "$upstream_bare"
git -C "$upstream_seed" branch -M main
git -C "$upstream_seed" push origin main --tags >/dev/null

git clone "$upstream_bare" "$fork_repo" >/dev/null 2>&1
git -C "$fork_repo" config user.name "Test User"
git -C "$fork_repo" config user.email "test@example.com"
printf 'local\n' >>"$fork_repo/README.md"
git -C "$fork_repo" commit -am "local change" >/dev/null
git -C "$fork_repo" tag v9.999.0

printf 'origin-ahead\n' >>"$upstream_seed/README.md"
git -C "$upstream_seed" commit -am "origin ahead" >/dev/null
git -C "$upstream_seed" push origin main >/dev/null

REPO_ROOT_OVERRIDE="$fork_repo" \
UPSTREAM_URL="$upstream_bare" \
UPGRADE_DATE="20260319" \
"$SCRIPT_PATH"

git -C "$fork_repo" show-ref --verify --quiet refs/heads/upstream-sync/v1.224.0
git -C "$fork_repo" show-ref --verify --quiet refs/heads/upgrade/nautilus-20260319-v1.224.0
current_branch="$(git -C "$fork_repo" branch --show-current)"
test "$current_branch" = "upgrade/nautilus-20260319-v1.224.0"
grep -q 'origin-ahead' "$fork_repo/README.md"

dirty_repo="$tmpdir/dirty"
git clone "$upstream_bare" "$dirty_repo" >/dev/null 2>&1
git -C "$dirty_repo" config user.name "Test User"
git -C "$dirty_repo" config user.email "test@example.com"
touch "$dirty_repo/UNTRACKED.tmp"
if REPO_ROOT_OVERRIDE="$dirty_repo" UPSTREAM_URL="$upstream_bare" UPGRADE_DATE="20260319" "$SCRIPT_PATH"; then
  echo "expected dirty worktree run to fail" >&2
  exit 1
fi

invalid_cherry_pick_repo="$tmpdir/invalid-cherry-pick"
git clone "$upstream_bare" "$invalid_cherry_pick_repo" >/dev/null 2>&1
git -C "$invalid_cherry_pick_repo" config user.name "Test User"
git -C "$invalid_cherry_pick_repo" config user.email "test@example.com"
if REPO_ROOT_OVERRIDE="$invalid_cherry_pick_repo" \
  UPSTREAM_URL="$upstream_bare" \
  UPGRADE_DATE="20260320" \
  CHERRY_PICK_COMMITS="README.md" \
  "$SCRIPT_PATH"; then
  echo "expected invalid cherry-pick identifiers to fail" >&2
  exit 1
fi

missing_cherry_pick_repo="$tmpdir/missing-cherry-pick"
git clone "$upstream_bare" "$missing_cherry_pick_repo" >/dev/null 2>&1
git -C "$missing_cherry_pick_repo" config user.name "Test User"
git -C "$missing_cherry_pick_repo" config user.email "test@example.com"
if REPO_ROOT_OVERRIDE="$missing_cherry_pick_repo" \
  UPSTREAM_URL="$upstream_bare" \
  UPGRADE_DATE="20260321" \
  CHERRY_PICK_COMMITS="0000000000000000000000000000000000000000" \
  "$SCRIPT_PATH"; then
  echo "expected missing cherry-pick commits to fail" >&2
  exit 1
fi
