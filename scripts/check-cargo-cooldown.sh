#!/bin/bash
# Flag crates in a Cargo.lock diff whose new version was published less than N days ago.
#
# Cargo has no built-in equivalent of uv's `exclude-newer`, so we approximate the
# Python cooldown policy here. Runs as a pre-commit hook and on demand:
#
#     scripts/check-cargo-cooldown.sh
#     scripts/check-cargo-cooldown.sh --days 7
#     scripts/check-cargo-cooldown.sh --base origin/develop
#
# Repeat --lock to gate more than one lockfile; it defaults to Cargo.lock. A
# crate version appearing in several locks is checked once.
#
# Policy lives in Cargo.toml [workspace.metadata.cooldown]. A crate listed under
# [workspace.metadata.cooldown.allow] passes despite being fresh, but only when
# .supply-chain/audits.toml carries an audit covering that exact version, as
# either `version = "X"` or `delta = "W -> X"`.
#
# Only registry-sourced packages are checked. Workspace members and git
# dependencies have no crates.io release, so bumping the workspace version is
# not a cooldown event.
#
# Exits 0 when every bumped crate is old enough, or allowed with a matching
# audit. Unverifiable versions fail closed; use `SKIP=cargo-cooldown` to commit
# without the gate.

set -euo pipefail

DAYS=""
BASE=HEAD
LOCKS=()
TIMEOUT=15
CARGO_TOML=Cargo.toml
AUDITS=.supply-chain/audits.toml

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
      LOCKS+=("$2")
      shift 2
      ;;
    --timeout)
      TIMEOUT="$2"
      shift 2
      ;;
    -h | --help)
      sed -n '2,25p' "$0" | sed 's/^# \{0,1\}//'
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

# Convert an ISO-8601 UTC timestamp to epoch seconds, trying GNU date then BSD.
# Fractional seconds are trimmed because BSD date cannot parse them.
iso_to_epoch() {
  local iso=${1:0:19}
  date -u -d "$iso" +%s 2> /dev/null ||
    date -j -u -f '%Y-%m-%dT%H:%M:%S' "$iso" +%s 2> /dev/null
}

# Render epoch seconds as an ISO-8601 UTC timestamp, trying GNU date then BSD.
epoch_to_iso() {
  date -u -d "@$1" +%Y-%m-%dT%H:%M:%SZ 2> /dev/null ||
    date -u -r "$1" +%Y-%m-%dT%H:%M:%SZ 2> /dev/null
}

# Read a scalar from a Cargo.toml table, matching the parsing style used by
# scripts/cargo-tool-version.sh.
read_metadata() {
  local table=$1 key=$2
  [[ -f "$CARGO_TOML" ]] || return 0
  awk -v table="[$table]" -v key="$key" '
    $0 == table { in_section=1; next }
    /^\[/ { in_section=0 }
    in_section && $1 == key { gsub(/[" ]/, "", $3); print $3; exit }
  ' "$CARGO_TOML"
}

# Report the allowed version for a crate, empty when it is not allowlisted.
allowed_version() {
  read_metadata "workspace.metadata.cooldown.allow" "$1"
}

# Succeed when audits.toml certifies this exact version of the crate, either as
# a full-version audit or as the target of a delta audit.
has_audit() {
  local crate=$1 version=$2
  [[ -f "$AUDITS" ]] || return 1
  awk -v header="[[audits.${crate}]]" -v ver="$version" '
    $0 == header { in_block=1; next }
    /^\[/ { in_block=0 }
    in_block && $0 == "version = \"" ver "\"" { found=1; exit }
    in_block && index($0, " -> " ver "\"") > 0 && $1 == "delta" { found=1; exit }
    END { exit !found }
  ' "$AUDITS"
}

if [[ -z "$DAYS" ]]; then
  DAYS=$(read_metadata "workspace.metadata.cooldown" "days")
fi
DAYS=${DAYS:-3}

if ((${#LOCKS[@]} == 0)); then
  LOCKS=(Cargo.lock)
fi

if ! now_secs=$(date -u +%s); then
  echo "Could not read the current time from date" >&2
  exit 2
fi
cutoff_secs=$((now_secs - DAYS * 86400))
if ! cutoff_iso=$(epoch_to_iso "$cutoff_secs"); then
  echo "Neither GNU nor BSD date could format a timestamp" >&2
  exit 2
fi

candidates=""
for lock in "${LOCKS[@]}"; do
  if [[ ! -f "$lock" ]]; then
    echo "Lock file not found: $lock" >&2
    exit 2
  fi

  # Walk the unified diff. Track the current [[package]] name across context
  # lines, then emit (name, new_version) when we see a +version line under it.
  # The block header itself may be on a context, '-', or '+' line depending on
  # insertion pattern, so accept all three prefixes.
  bumped=$(git diff "$BASE" -- "$lock" | awk '
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
  [[ -n "$bumped" ]] || continue

  # Registry-sourced packages in the resulting lock. Reading the lock rather
  # than the diff keeps the source classification reliable when the unchanged
  # `source` line falls outside the diff context.
  registry=$(awk '
    /^\[\[package\]\]/ { name=""; version=""; next }
    /^name = "/ { name=$0; sub(/^name = "/, "", name); sub(/"$/, "", name); next }
    /^version = "/ { version=$0; sub(/^version = "/, "", version); sub(/"$/, "", version); next }
    /^source = "registry\+/ {
      if (name != "" && version != "") print name " " version
    }
  ' "$lock")

  while IFS= read -r entry; do
    [[ -z "$entry" ]] && continue
    if grep -Fxq "$entry" <<< "$registry"; then
      candidates+="${entry}"$'\n'
    fi
  done <<< "$bumped"
done

# One crate version can appear in several locks; check it once.
candidates=$(printf '%s' "$candidates" | awk 'NF' | sort -u)

if [[ -z "$candidates" ]]; then
  echo "No new registry crate versions vs $BASE."
  exit 0
fi

count=$(printf '%s\n' "$candidates" | wc -l | tr -d '[:space:]')
echo "Checking ${count} bumped crate version(s) against ${DAYS}-day cooldown"
echo "Cutoff: ${cutoff_iso}"
echo

printf '%-32s %-14s %-22s %s\n' "crate" "version" "published" "age"
printf -- '-%.0s' {1..82}
printf '\n'

fresh_lines=()
unaudited_lines=()
lookup_lines=()
parse_lines=()
allowed_lines=()

while IFS=' ' read -r name version; do
  [[ -z "$name" ]] && continue
  url="https://crates.io/api/v1/crates/${name}/${version}"
  if ! json=$(curl -fsSL --max-time "$TIMEOUT" -A "nautilus-cargo-cooldown/1.0" "$url" 2> /dev/null); then
    printf '%-32s %-14s LOOKUP FAILED\n' "$name" "$version"
    lookup_lines+=("${name} ${version}: registry request failed")
    continue
  fi
  published=$(printf '%s' "$json" | jq -r '.version.created_at // empty')
  if [[ -z "$published" ]]; then
    printf '%-32s %-14s NO DATE FIELD\n' "$name" "$version"
    parse_lines+=("${name} ${version}: no created_at in response")
    continue
  fi
  if ! pub_secs=$(iso_to_epoch "$published"); then
    printf '%-32s %-14s UNPARSEABLE DATE\n' "$name" "$version"
    parse_lines+=("${name} ${version}: could not parse ${published}")
    continue
  fi
  age_days=$(((now_secs - pub_secs) / 86400))
  flag=""
  if ((pub_secs > cutoff_secs)); then
    if [[ "$(allowed_version "$name")" == "$version" ]]; then
      if has_audit "$name" "$version"; then
        flag="  ** FRESH (allowed, audited)"
        allowed_lines+=("${name} ${version}")
      else
        flag="  ** FRESH (allowed, NO AUDIT)"
        unaudited_lines+=("${name} ${version}")
      fi
    else
      flag="  ** FRESH"
      fresh_lines+=("${name} ${version} (published ${published})")
    fi
  fi
  printf '%-32s %-14s %-22s %sd%s\n' "$name" "$version" "${published:0:19}" "$age_days" "$flag"
done <<< "$candidates"

echo

exit_code=0

if ((${#lookup_lines[@]} > 0)); then
  echo "FAIL: ${#lookup_lines[@]} crate(s) could not be reached on crates.io:"
  for line in "${lookup_lines[@]}"; do
    echo "  - ${line}"
  done
  echo "  Retry with the registry reachable, or use SKIP=cargo-cooldown to bypass the gate."
  exit_code=1
fi

if ((${#parse_lines[@]} > 0)); then
  echo "FAIL: ${#parse_lines[@]} crate(s) returned a release date this host could not read:"
  for line in "${parse_lines[@]}"; do
    echo "  - ${line}"
  done
  exit_code=1
fi

if ((${#unaudited_lines[@]} > 0)); then
  echo "FAIL: ${#unaudited_lines[@]} allowed crate(s) missing a cargo-vet audit:"
  for line in "${unaudited_lines[@]}"; do
    echo "  - ${line}"
  done
  echo "  Review the published delta with 'cargo vet diff <crate> <old> <new>',"
  echo "  then record it in ${AUDITS}."
  exit_code=1
fi

if ((${#fresh_lines[@]} > 0)); then
  echo "FAIL: ${#fresh_lines[@]} crate(s) within the ${DAYS}-day cooldown:"
  for line in "${fresh_lines[@]}"; do
    echo "  - ${line}"
  done
  echo "  Hold the bump, or allow it in Cargo.toml"
  echo "  [workspace.metadata.cooldown.allow] with a matching audit."
  exit_code=1
fi

if ((exit_code == 0)); then
  if ((${#allowed_lines[@]} > 0)); then
    echo "OK: ${#allowed_lines[@]} audited exception(s) inside the ${DAYS}-day cooldown:"
    for line in "${allowed_lines[@]}"; do
      echo "  - ${line}"
    done
  else
    echo "OK: all ${count} bumped crate(s) are at least ${DAYS} days old."
  fi
fi

exit "$exit_code"
