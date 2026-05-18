#!/bin/bash
# Flag crates in a Cargo.lock diff whose new version was published less than N days ago.
#
# Cargo has no built-in equivalent of uv's `exclude-newer`, so we approximate the
# Python cooldown policy with a manual check on demand. Run before promoting a
# `cargo update` to a commit:
#
#     scripts/check-cargo-cooldown.sh
#     scripts/check-cargo-cooldown.sh --days 7
#     scripts/check-cargo-cooldown.sh --base origin/develop
#
# Exits 0 if every bumped crate is at least `--days` days old, 1 otherwise.
# Lookup failures count as failures (fail closed) so an offline crates.io does
# not silently mask a fresh bump.

set -euo pipefail

DAYS=3
BASE=HEAD
LOCK=Cargo.lock
TIMEOUT=15

while [[ $# -gt 0 ]]; do
  case "$1" in
    --days)
      DAYS="$2"
      shift 2
      ;;
    --base)
      BASE="$2"
      shift 2
      ;;
    --lock)
      LOCK="$2"
      shift 2
      ;;
    --timeout)
      TIMEOUT="$2"
      shift 2
      ;;
    -h | --help)
      sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

for tool in git curl jq awk date; do
  command -v "$tool" > /dev/null || {
    echo "Required tool not on PATH: $tool" >&2
    exit 2
  }
done

# Walk the unified diff. Track the current [[package]] name across context lines,
# then emit (name, new_version) when we see a +version line under it. The block
# header itself may be on a context, '-', or '+' line depending on insertion
# pattern, so accept all three prefixes.
bumped=$(git diff "$BASE" -- "$LOCK" | awk '
  /^[+ -]\[\[package\]\]/ { name=""; next }
  /^[+ -]name = "/ {
    n=$0
    sub(/^[+ -]name = "/, "", n)
    sub(/"$/, "", n)
    name=n
    next
  }
  /^\+version = "/ && name != "" {
    v=$0
    sub(/^\+version = "/, "", v)
    sub(/"$/, "", v)
    print name " " v
  }
')

if [[ -z "$bumped" ]]; then
  echo "No new crate versions vs $BASE."
  exit 0
fi

now_secs=$(date -u +%s)
cutoff_secs=$((now_secs - DAYS * 86400))
cutoff_iso=$(date -u -d "@${cutoff_secs}" -Iseconds)

count=$(echo "$bumped" | wc -l | tr -d '[:space:]')
echo "Checking ${count} bumped crate version(s) against ${DAYS}-day cooldown"
echo "Cutoff: ${cutoff_iso}"
echo

printf '%-32s %-14s %-22s %s\n' "crate" "version" "published" "age"
printf -- '-%.0s' {1..82}
printf '\n'

fresh_lines=()
skipped_lines=()

while IFS=' ' read -r name version; do
  [[ -z "$name" ]] && continue
  url="https://crates.io/api/v1/crates/${name}/${version}"
  if ! json=$(curl -fsSL --max-time "$TIMEOUT" -A "nautilus-cargo-cooldown/1.0" "$url" 2> /dev/null); then
    printf '%-32s %-14s LOOKUP FAILED\n' "$name" "$version"
    skipped_lines+=("${name} ${version}: HTTP request failed")
    continue
  fi
  published=$(printf '%s' "$json" | jq -r '.version.created_at // empty')
  if [[ -z "$published" ]]; then
    printf '%-32s %-14s NO DATE FIELD\n' "$name" "$version"
    skipped_lines+=("${name} ${version}: no created_at in response")
    continue
  fi
  if ! pub_secs=$(date -u -d "$published" +%s 2> /dev/null); then
    printf '%-32s %-14s UNPARSEABLE DATE\n' "$name" "$version"
    skipped_lines+=("${name} ${version}: could not parse ${published}")
    continue
  fi
  age_days=$(((now_secs - pub_secs) / 86400))
  flag=""
  if ((pub_secs > cutoff_secs)); then
    flag="  ** FRESH"
    fresh_lines+=("${name} ${version} (published ${published})")
  fi
  printf '%-32s %-14s %-22s %sd%s\n' "$name" "$version" "${published:0:19}" "$age_days" "$flag"
done <<< "$bumped"

echo

exit_code=0

if ((${#skipped_lines[@]} > 0)); then
  echo "FAIL: ${#skipped_lines[@]} crate(s) could not be verified:"
  for line in "${skipped_lines[@]}"; do
    echo "  - ${line}"
  done
  exit_code=1
fi

if ((${#fresh_lines[@]} > 0)); then
  echo "FAIL: ${#fresh_lines[@]} crate(s) within the ${DAYS}-day cooldown:"
  for line in "${fresh_lines[@]}"; do
    echo "  - ${line}"
  done
  exit_code=1
fi

if ((exit_code == 0)); then
  echo "OK: all ${count} bumped crate(s) are at least ${DAYS} days old."
fi

exit "$exit_code"
