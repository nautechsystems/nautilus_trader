#!/bin/bash
# Flag crates in a Cargo.lock diff whose new version was published less than N days ago.
#
# Cargo has no built-in equivalent of uv's `exclude-newer`, so this script
# enforces the Rust release cooldown. Runs as a pre-commit hook and on demand:
#
#     scripts/check-cargo-cooldown.sh
#     scripts/check-cargo-cooldown.sh --days 7
#     scripts/check-cargo-cooldown.sh --base origin/develop
#     scripts/check-cargo-cooldown.sh --fix
#
# CI uses CHANGED_BASE_SHA when it resolves. New-branch sentinels and unreachable
# force-push bases fall back to the origin/develop merge base, then HEAD^.
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
# By default this is a read-only gate. `--fix` asks Cargo to restore the prior
# version of each fresh, unallowed upgrade when the diff identifies exactly one
# prior version. The update wrapper snapshots the lockfiles before using this
# mode so any failed or ambiguous repair can restore the whole transaction.
#
# Exits 0 when every bumped crate is old enough, or allowed with a matching
# audit. Unverifiable versions fail closed; use `SKIP=cargo-cooldown` to commit
# without the gate.

set -euo pipefail

DAYS=""
BASE=HEAD
BASE_EXPLICIT=false
LOCKS=()
TIMEOUT=15
CARGO_TOML=Cargo.toml
AUDITS=.supply-chain/audits.toml
FIX=false
SNAPSHOT_DIR=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --days)
      DAYS="$2"
      shift 2
      ;;
    --base)
      BASE="$2"
      BASE_EXPLICIT=true
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
    --fix)
      FIX=true
      shift
      ;;
    --snapshot-dir)
      SNAPSHOT_DIR="$2"
      shift 2
      ;;
    -h | --help)
      sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'
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

# Probe with a known timestamp because BSD date uses `-d` for a different
# option and can succeed without parsing the supplied timestamp.
if [[ $(date -u -d "1970-01-01T00:00:01Z" +%s 2> /dev/null) == "1" ]]; then
  DATE_KIND=gnu
else
  DATE_KIND=bsd
fi

if [[ "$BASE_EXPLICIT" == false && -n "${CHANGED_BASE_SHA:-}" ]]; then
  resolved_base=""
  current_head=$(git rev-parse HEAD)
  if [[ "$CHANGED_BASE_SHA" != "0000000000000000000000000000000000000000" ]]; then
    resolved_base=$(git merge-base "$CHANGED_BASE_SHA" HEAD 2> /dev/null || true)
  fi
  if [[ -z "$resolved_base" ]]; then
    resolved_base=$(git merge-base origin/develop HEAD 2> /dev/null || true)
    if [[ "$resolved_base" == "$current_head" ]]; then
      resolved_base=""
    fi
  fi
  if [[ -n "$resolved_base" ]]; then
    BASE=$resolved_base
  elif git rev-parse --verify --quiet HEAD^ > /dev/null; then
    BASE=HEAD^
  else
    echo "Could not resolve a comparison base from CHANGED_BASE_SHA=$CHANGED_BASE_SHA" >&2
    exit 2
  fi
fi

if [[ "$FIX" == true ]]; then
  command -v cargo > /dev/null || {
    echo "Required tool not on PATH: cargo" >&2
    exit 2
  }
  if [[ -n "$SNAPSHOT_DIR" ]]; then
    for tool in diff cmp; do
      command -v "$tool" > /dev/null || {
        echo "Required tool not on PATH: $tool" >&2
        exit 2
      }
    done
  fi
elif [[ -n "$SNAPSHOT_DIR" ]]; then
  echo "--snapshot-dir requires --fix" >&2
  exit 2
fi

# Convert an ISO-8601 UTC timestamp to epoch seconds with GNU or BSD date.
# Fractional seconds are trimmed because BSD date cannot parse them.
iso_to_epoch() {
  local iso=${1:0:19}
  if [[ "$DATE_KIND" == gnu ]]; then
    date -u -d "$iso" +%s 2> /dev/null
  else
    date -j -u -f '%Y-%m-%dT%H:%M:%S' "$iso" +%s 2> /dev/null
  fi
}

# Render epoch seconds as an ISO-8601 UTC timestamp with GNU or BSD date.
epoch_to_iso() {
  if [[ "$DATE_KIND" == gnu ]]; then
    date -u -d "@$1" +%Y-%m-%dT%H:%M:%SZ 2> /dev/null
  else
    date -u -r "$1" +%Y-%m-%dT%H:%M:%SZ 2> /dev/null
  fi
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

manifest_for_lock() {
  local lock=$1 lock_dir
  if [[ "$(basename "$lock")" != "Cargo.lock" ]]; then
    echo "--fix requires a Cargo.lock path, received: $lock" >&2
    return 1
  fi
  lock_dir=$(dirname "$lock")
  if [[ "$lock_dir" == "." ]]; then
    printf '%s\n' "$CARGO_TOML"
  else
    printf '%s/Cargo.toml\n' "$lock_dir"
  fi
}

validate_lock_manifests() {
  local lock manifest

  for lock in "${LOCKS[@]}"; do
    manifest=$(manifest_for_lock "$lock") || return 1
    if [[ ! -f "$manifest" ]]; then
      echo "Cargo manifest not found for $lock: $manifest" >&2
      return 1
    fi
    if ! cargo metadata \
      --manifest-path "$manifest" \
      --locked \
      --no-deps \
      --format-version 1 > /dev/null; then
      echo "Cargo metadata validation failed for $manifest" >&2
      return 1
    fi
  done
}

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
candidate_entries=""
for lock in "${LOCKS[@]}"; do
  if [[ ! -f "$lock" ]]; then
    echo "Lock file not found: $lock" >&2
    exit 2
  fi

  lock_diff=$(git diff "$BASE" -- "$lock")
  if [[ "$FIX" == true && -n "$SNAPSHOT_DIR" ]]; then
    previous_lock="${SNAPSHOT_DIR}/${lock}"
    if [[ ! -f "$previous_lock" ]]; then
      echo "Lock snapshot not found: $previous_lock" >&2
      exit 2
    fi
    snapshot_status=0
    snapshot_diff=$(diff -u "$previous_lock" "$lock") || snapshot_status=$?
    if ((snapshot_status > 1)); then
      echo "Could not compare $lock with its pre-update snapshot" >&2
      exit 2
    fi
    lock_diff+=$'\n'"$snapshot_diff"
  fi

  # Walk the unified diffs. Track the current [[package]] name across context
  # lines, then emit (name, new_version) when we see a +version line under it.
  # The block header itself may be on a context, '-', or '+' line depending on
  # insertion pattern, so accept all three prefixes.
  bumped=$(printf '%s\n' "$lock_diff" | awk '
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
      read -r entry_name entry_version <<< "$entry"
      candidate_entries+="${lock}|${entry_name}|${entry_version}"$'\n'
    fi
  done <<< "$bumped"
done

# One crate version can appear in several locks; check it once.
candidates=$(printf '%s' "$candidates" | awk 'NF' | sort -u)
candidate_entries=$(printf '%s' "$candidate_entries" | awk 'NF' | sort -u)

if [[ -z "$candidates" ]]; then
  echo "No new registry crate versions vs $BASE."
  if [[ "$FIX" == true ]]; then
    validate_lock_manifests
    echo "No cooldown rollback required"
    echo "Cargo cooldown repair complete"
  fi
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
fresh_versions=()
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
      fresh_versions+=("${name} ${version}")
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

if ((${#fresh_lines[@]} > 0)) && [[ "$FIX" == false ]]; then
  echo "FAIL: ${#fresh_lines[@]} crate(s) within the ${DAYS}-day cooldown:"
  for line in "${fresh_lines[@]}"; do
    echo "  - ${line}"
  done
  echo "  Hold the bump, or allow it in Cargo.toml"
  echo "  [workspace.metadata.cooldown.allow] with a matching audit."
  exit_code=1
fi

if ((exit_code != 0)); then
  exit "$exit_code"
fi

if [[ "$FIX" == false ]]; then
  if ((${#allowed_lines[@]} > 0)); then
    echo "OK: ${#allowed_lines[@]} audited exception(s) inside the ${DAYS}-day cooldown:"
    for line in "${allowed_lines[@]}"; do
      echo "  - ${line}"
    done
  else
    echo "OK: all ${count} bumped crate(s) are at least ${DAYS} days old."
  fi
  exit 0
fi

if ((${#fresh_versions[@]} == 0)); then
  validate_lock_manifests
  if ((${#allowed_lines[@]} > 0)); then
    echo "OK: ${#allowed_lines[@]} audited exception(s) inside the ${DAYS}-day cooldown:"
    for line in "${allowed_lines[@]}"; do
      echo "  - ${line}"
    done
  else
    echo "OK: all ${count} bumped crate(s) are at least ${DAYS} days old."
  fi
  echo "No cooldown rollback required"
  echo "Cargo cooldown repair complete"
  exit 0
fi

is_fresh_version() {
  local wanted_name=$1 wanted_version=$2 fresh
  for fresh in "${fresh_versions[@]}"; do
    if [[ "$fresh" == "$wanted_name $wanted_version" ]]; then
      return 0
    fi
  done
  return 1
}

package_versions() {
  local lock=$1 target=$2
  awk -v target="$target" '
    /^\[\[package\]\]/ { name=""; next }
    /^name = "/ {
      name=$0
      sub(/^name = "/, "", name)
      sub(/"$/, "", name)
      next
    }
    /^version = "/ && name == target {
      version=$0
      sub(/^version = "/, "", version)
      sub(/"$/, "", version)
      print version
    }
  ' "$lock" | sort -u
}

versions_removed_from_lock() {
  local previous_lock=$1 lock=$2 name=$3
  local previous_versions current_versions version
  previous_versions=$(package_versions "$previous_lock" "$name")
  current_versions=$(package_versions "$lock" "$name")
  while IFS= read -r version; do
    [[ -n "$version" ]] || continue
    if ! grep -Fxq "$version" <<< "$current_versions"; then
      printf '%s\n' "$version"
    fi
  done <<< "$previous_versions"
}

versions_removed_from_base() {
  local lock=$1 name=$2
  git diff "$BASE" -- "$lock" | awk -v target="$name" '
    /^[+ -]\[\[package\]\]/ { name=""; next }
    /^[+ -]name = "/ {
      name=$0
      sub(/^[+ -]name = "/, "", name)
      sub(/"$/, "", name)
      next
    }
    /^-version = "/ && name == target {
      version=$0
      sub(/^-version = "/, "", version)
      sub(/"$/, "", version)
      print version
    }
  ' | sort -u
}

fresh_versions_for_lock_name() {
  local wanted_lock=$1 wanted_name=$2
  local entry_lock name version
  while IFS='|' read -r entry_lock name version; do
    [[ "$entry_lock" == "$wanted_lock" && "$name" == "$wanted_name" ]] || continue
    if is_fresh_version "$name" "$version"; then
      printf '%s\n' "$version"
    fi
  done <<< "$candidate_entries"
}

package_version_occurrences() {
  local lock=$1 target_name=$2 target_version=$3
  awk -v target_name="$target_name" -v target_version="$target_version" '
    /^\[\[package\]\]/ {
      if (name == target_name && version == target_version) count++
      name=""
      version=""
      next
    }
    /^name = "/ {
      name=$0
      sub(/^name = "/, "", name)
      sub(/"$/, "", name)
      next
    }
    /^version = "/ {
      version=$0
      sub(/^version = "/, "", version)
      sub(/"$/, "", version)
    }
    END {
      if (name == target_name && version == target_version) count++
      print count + 0
    }
  ' "$lock"
}

package_identities() {
  local lock=$1
  awk '
    function emit() {
      if (name != "" && version != "") print name "|" version "|" source
    }
    /^\[\[package\]\]/ {
      emit()
      name=""
      version=""
      source=""
      next
    }
    /^name = "/ {
      name=$0
      sub(/^name = "/, "", name)
      sub(/"$/, "", name)
      next
    }
    /^version = "/ {
      version=$0
      sub(/^version = "/, "", version)
      sub(/"$/, "", version)
      next
    }
    /^source = "/ {
      source=$0
      sub(/^source = "/, "", source)
      sub(/"$/, "", source)
    }
    END { emit() }
  ' "$lock" | LC_ALL=C sort
}

repair_locks=()
repair_names=()
repair_new_versions=()
repair_old_versions=()
repair_manifests=()
repair_errors=()

while IFS='|' read -r lock name new_version; do
  [[ -n "$lock" ]] || continue
  is_fresh_version "$name" "$new_version" || continue

  fresh_new_versions=$(fresh_versions_for_lock_name "$lock" "$name" | sort -u)
  first_new_version=$(printf '%s\n' "$fresh_new_versions" | awk 'NF { print; exit }')
  [[ "$new_version" == "$first_new_version" ]] || continue
  new_count=$(printf '%s\n' "$fresh_new_versions" | awk 'NF { count++ } END { print count + 0 }')
  if [[ "$new_count" != "1" ]]; then
    repair_errors+=("${name} in ${lock}: ${new_count} fresh versions have no unique rollback pairing")
    continue
  fi
  identity_count=$(package_version_occurrences "$lock" "$name" "$new_version")
  if [[ "$identity_count" != "1" ]]; then
    repair_errors+=("${name} ${new_version} in ${lock}: ${identity_count} package identities are ambiguous")
    continue
  fi

  if ! manifest=$(manifest_for_lock "$lock"); then
    repair_errors+=("${name} ${new_version} in ${lock}: unsupported lock path")
    continue
  fi
  if [[ ! -f "$manifest" ]]; then
    repair_errors+=("${name} ${new_version} in ${lock}: missing ${manifest}")
    continue
  fi

  if [[ -n "$SNAPSHOT_DIR" ]]; then
    previous_lock="${SNAPSHOT_DIR}/${lock}"
    if [[ ! -f "$previous_lock" ]]; then
      repair_errors+=("${name} ${new_version} in ${lock}: missing snapshot ${previous_lock}")
      continue
    fi
    if package_versions "$previous_lock" "$name" | grep -Fxq "$new_version"; then
      old_versions=$(versions_removed_from_base "$lock" "$name")
    else
      old_versions=$(versions_removed_from_lock "$previous_lock" "$lock" "$name")
    fi
  else
    old_versions=$(versions_removed_from_base "$lock" "$name")
  fi
  old_count=$(printf '%s\n' "$old_versions" | awk 'NF { count++ } END { print count + 0 }')
  if [[ "$old_count" != "1" ]]; then
    if [[ "$old_count" == "0" ]]; then
      reason="no prior version is present in the diff"
    else
      reason="the diff contains ${old_count} possible prior versions"
    fi
    repair_errors+=("${name} ${new_version} in ${lock}: ${reason}")
    continue
  fi

  old_version=$(printf '%s\n' "$old_versions" | awk 'NF { print; exit }')
  repair_locks+=("$lock")
  repair_names+=("$name")
  repair_new_versions+=("$new_version")
  repair_old_versions+=("$old_version")
  repair_manifests+=("$manifest")
done <<< "$candidate_entries"

if ((${#repair_errors[@]} > 0)); then
  echo "FAIL: cannot safely repair ${#repair_errors[@]} cooldown violation(s):"
  for line in "${repair_errors[@]}"; do
    echo "  - ${line}"
  done
  exit 1
fi

rollback_lines=()
repair_pending=()
repair_failures=()
for ((i = 0; i < ${#repair_locks[@]}; i++)); do
  repair_pending+=(true)
  repair_failures+=("")
done
remaining=${#repair_locks[@]}

# The initial update normally primes Cargo's index cache. Standalone fixes fall
# back to the network only when a complete offline pass cannot make progress.
repair_online=false
while ((remaining > 0)); do
  progress=false
  for ((i = 0; i < ${#repair_locks[@]}; i++)); do
    [[ "${repair_pending[$i]}" == true ]] || continue

    lock=${repair_locks[$i]}
    name=${repair_names[$i]}
    new_version=${repair_new_versions[$i]}
    old_version=${repair_old_versions[$i]}
    manifest=${repair_manifests[$i]}

    new_occurrences=$(package_version_occurrences "$lock" "$name" "$new_version")
    if [[ "$new_occurrences" == "0" ]]; then
      old_occurrences=$(package_version_occurrences "$lock" "$name" "$old_version")
      if [[ "$old_occurrences" == "1" ]]; then
        rollback_lines+=("${name} ${new_version} -> ${old_version} (${lock}; resolved by earlier Cargo update)")
        repair_pending[i]=false
        remaining=$((remaining - 1))
        progress=true
        continue
      fi
      echo "FAIL: ${name} ${new_version} disappeared without resolving uniquely to ${old_version} in ${lock}." >&2
      exit 1
    elif [[ "$new_occurrences" != "1" ]]; then
      echo "FAIL: ${name} ${new_version} now has ${new_occurrences} ambiguous package identities in ${lock}." >&2
      exit 1
    fi

    cargo_args=(update)
    if [[ "$repair_online" == false ]]; then
      cargo_args+=(--offline)
    fi
    cargo_args+=(
      --manifest-path "$manifest"
      -p "${name}@${new_version}"
      --precise "$old_version"
    )
    if cargo_output=$(cargo "${cargo_args[@]}" 2>&1); then
      printf '%s\n' "$cargo_output" >&2
      new_occurrences=$(package_version_occurrences "$lock" "$name" "$new_version")
      old_occurrences=$(package_version_occurrences "$lock" "$name" "$old_version")
      if [[ "$new_occurrences" != "0" || "$old_occurrences" != "1" ]]; then
        echo "FAIL: Cargo did not resolve ${name} uniquely from ${new_version} to ${old_version} in ${lock}." >&2
        exit 1
      fi
      rollback_lines+=("${name} ${new_version} -> ${old_version} (${lock})")
      repair_pending[i]=false
      remaining=$((remaining - 1))
      progress=true
    else
      repair_failures[i]=$cargo_output
    fi
  done

  if ((remaining > 0)); then
    if [[ "$progress" == false ]]; then
      if [[ "$repair_online" == false ]]; then
        echo "Offline Cargo rollback made no progress; retrying ${remaining} remaining rollback(s) with network access" >&2
        repair_online=true
        continue
      fi
      echo "FAIL: Cargo could not make progress on ${remaining} cooldown rollback(s):" >&2
      for ((i = 0; i < ${#repair_locks[@]}; i++)); do
        [[ "${repair_pending[$i]}" == true ]] || continue
        echo "  - ${repair_names[$i]} ${repair_new_versions[$i]} -> ${repair_old_versions[$i]} (${repair_locks[$i]})" >&2
        if [[ -n "${repair_failures[$i]}" ]]; then
          printf '%s\n' "${repair_failures[$i]}" >&2
        fi
      done
      exit 1
    elif [[ "$repair_online" == true ]]; then
      repair_online=false
    fi
  fi
done

normalization_lines=()
normalization_count=0
if [[ -n "$SNAPSHOT_DIR" ]]; then
  for lock in "${LOCKS[@]}"; do
    previous_lock="${SNAPSHOT_DIR}/${lock}"
    if cmp -s "$previous_lock" "$lock"; then
      continue
    fi
    current_identities=$(package_identities "$lock")
    previous_identities=$(package_identities "$previous_lock")
    if [[ "$current_identities" == "$previous_identities" ]]; then
      cp -p "$previous_lock" "$lock"
      normalization_lines+=("$lock")
      normalization_count=$((normalization_count + 1))
    fi
  done
fi

validate_lock_manifests

verify_args=(--days "$DAYS" --base "$BASE" --timeout "$TIMEOUT")
for lock in "${LOCKS[@]}"; do
  verify_args+=(--lock "$lock")
done
if ! bash "$0" "${verify_args[@]}"; then
  echo "FAIL: cooldown validation still fails after Cargo rollback." >&2
  exit 1
fi

echo
if ((normalization_count > 0)); then
  echo "Restored exact pre-update content for ${normalization_count} lockfile(s):"
  for line in "${normalization_lines[@]}"; do
    echo "  - ${line}"
  done
fi
echo "Rolled back ${#rollback_lines[@]} fresh lockfile update(s):"
for line in "${rollback_lines[@]}"; do
  echo "  - ${line}"
done
echo "Cargo cooldown repair complete"
