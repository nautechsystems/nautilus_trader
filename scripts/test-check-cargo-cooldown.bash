#!/usr/bin/env bash
set -euo pipefail

# Companion test for scripts/check-cargo-cooldown.sh.
#
# Builds a throwaway Git repository per case, then drives the script with a fake
# crates.io on PATH so no case needs network access. One case swaps in a BSD
# style `date` that rejects GNU `-d` to prove the timestamp fallback works on
# macOS.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECK_SCRIPT="${SCRIPT_DIR}/check-cargo-cooldown.sh"
REAL_DATE="$(command -v date)"

for required in git awk jq curl date grep; do
  command -v "$required" > /dev/null || {
    echo "Required test command not on PATH: $required" >&2
    exit 1
  }
done

test_root="$(mktemp -d "${TMPDIR:-/tmp}/nautilus-cooldown-test.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT

fake_bin="${test_root}/bin"
curl_only_bin="${test_root}/bin-curl-only"
mkdir -p "$fake_bin" "$curl_only_bin"

cat > "${fake_bin}/curl" << 'FAKE_CURL'
#!/usr/bin/env bash
set -u
url=""
for arg in "$@"; do
  case "$arg" in
    https://*) url="$arg" ;;
  esac
done
crate="${url%/*}"
crate="${crate##*/}"
version="${url##*/}"
line="$(grep -E "^${crate} ${version} " "$FAKE_CRATES_FIXTURE" 2> /dev/null || true)"
if [[ -z "$line" ]]; then
  exit 22
fi
printf '{"version":{"created_at":"%s"}}\n' "$(printf '%s' "$line" | awk '{print $3}')"
FAKE_CURL
chmod +x "${fake_bin}/curl"

cat > "${fake_bin}/cargo" << 'FAKE_CARGO'
#!/usr/bin/env bash
set -u

printf '%s\n' "$*" >> "${FAKE_CARGO_LOG:-/dev/null}"

command_name=${1:-}
shift || true
case "$command_name" in
  update)
    manifest="Cargo.toml"
    package=""
    precise=""
    offline=false
    while (($# > 0)); do
      case "$1" in
        --offline)
          offline=true
          shift
          ;;
        --manifest-path)
          manifest=$2
          shift 2
          ;;
        -p)
          package=$2
          shift 2
          ;;
        --precise)
          precise=$2
          shift 2
          ;;
        *) shift ;;
      esac
    done
    if [[ -z "$package" || -z "$precise" ]]; then
      echo "fake cargo update requires -p and --precise" >&2
      exit 2
    fi
    if [[ "${FAKE_CARGO_FAIL_PACKAGE:-}" == "$package" ]]; then
      exit 1
    fi
    if [[ "$offline" == true &&
      "${FAKE_CARGO_FAIL_OFFLINE_PACKAGE:-}" == "$package" ]]; then
      exit 1
    fi
    crate=${package%@*}
    current=${package##*@}
    lock="$(dirname "$manifest")/Cargo.lock"
    tmp="${lock}.tmp"
    if ! awk -v crate="$crate" -v current="$current" -v precise="$precise" '
      /^\[\[package\]\]/ { name="" }
      /^name = "/ {
        name=$0
        sub(/^name = "/, "", name)
        sub(/"$/, "", name)
      }
      /^version = "/ && name == crate {
        version=$0
        sub(/^version = "/, "", version)
        sub(/"$/, "", version)
        if (version == current) {
          print "version = \"" precise "\""
          changed=1
          next
        }
      }
      { print }
      END { if (!changed) exit 3 }
    ' "$lock" > "$tmp"; then
      rm -f "$tmp"
      exit 1
    fi
    mv "$tmp" "$lock"
    ;;
  metadata)
    if [[ "${FAKE_CARGO_METADATA_FAIL:-0}" == "1" ]]; then
      exit 1
    fi
    ;;
  *)
    echo "unexpected fake cargo command: $command_name" >&2
    exit 2
    ;;
esac
FAKE_CARGO
chmod +x "${fake_bin}/cargo"

# BSD style date: rejects GNU `-d`, serves `-r` and `-j -u -f` via the real date.
cat > "${fake_bin}/date" << 'FAKE_DATE'
#!/usr/bin/env bash
set -u
case " $* " in
  *" -d "*)
    echo "date: illegal option -- d" >&2
    exit 1
    ;;
esac
if [[ "${1:-}" == "-u" && "${2:-}" == "-r" ]]; then
  exec "$REAL_DATE" -u -d "@$3" "$4"
fi
if [[ "${1:-}" == "-j" && "${2:-}" == "-u" && "${3:-}" == "-f" ]]; then
  exec "$REAL_DATE" -u -d "$5" "$6"
fi
exec "$REAL_DATE" "$@"
FAKE_DATE
chmod +x "${fake_bin}/date"

cp "${fake_bin}/curl" "${fake_bin}/cargo" "$curl_only_bin"

fixture="${test_root}/crates.txt"
cargo_log="${test_root}/cargo.log"
export FAKE_CRATES_FIXTURE="$fixture"
export FAKE_CARGO_LOG="$cargo_log"
export REAL_DATE

# Test controls must not inherit state from a developer's shell
unset CHANGED_BASE_SHA
unset FAKE_CARGO_FAIL_PACKAGE FAKE_CARGO_FAIL_OFFLINE_PACKAGE FAKE_CARGO_METADATA_FAIL

fresh_date="$("$REAL_DATE" -u +%Y-%m-%dT%H:%M:%SZ)"
old_date="2020-01-01T00:00:00Z"

failures=0

# Write a Cargo.lock holding one registry package plus one workspace member.
write_lock() {
  local registry_crate=$1 registry_version=$2 workspace_version=$3
  cat > "${repo}/Cargo.lock" << LOCK
version = 4

[[package]]
name = "nautilus-core"
version = "${workspace_version}"
dependencies = [
 "${registry_crate}",
]

[[package]]
name = "${registry_crate}"
version = "${registry_version}"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "0000000000000000000000000000000000000000000000000000000000000000"
LOCK
}

write_cargo_toml() {
  local allow_block=${1:-}
  {
    printf '[workspace]\nmembers = []\n\n'
    printf '[workspace.metadata.cooldown]\ndays = 3\n'
    if [[ -n "$allow_block" ]]; then
      printf '\n[workspace.metadata.cooldown.allow]\n%s\n' "$allow_block"
    fi
  } > "${repo}/Cargo.toml"
}

write_audits() {
  local body=${1:-}
  mkdir -p "${repo}/.supply-chain"
  printf '%s\n' "$body" > "${repo}/.supply-chain/audits.toml"
}

# Fresh repo with a committed baseline lock, so `git diff HEAD` sees the change.
setup_repo() {
  repo="${test_root}/repo-${1}"
  mkdir -p "$repo"
  git -C "$repo" init --quiet
  git -C "$repo" config user.email test@example.com
  git -C "$repo" config user.name "Test"
  # Contributors sign commits, and a throwaway repo has no usable key.
  git -C "$repo" config commit.gpgsign false
  write_cargo_toml
  write_audits ""
  write_lock "serde" "1.0.0" "0.1.0"
  git -C "$repo" add -A
  git -C "$repo" commit --quiet -m baseline
}

# Only the BSD case shadows `date`, so every other case exercises the GNU branch
# that CI and most contributors actually take.
run_check() {
  local bin_path="$fake_bin"
  if [[ "${USE_BSD_DATE:-0}" != "1" ]]; then
    bin_path="$curl_only_bin"
  fi
  (cd "$repo" && PATH="${bin_path}:${PATH}" bash "$CHECK_SCRIPT" "$@" 2>&1)
}

expect() {
  local label=$1 expected_status=$2 expected_text=$3
  shift 3
  local actual_status=0
  local output=""
  # Keep the `||` outside the substitution so the status lands in this scope.
  output="$(run_check "$@")" || actual_status=$?
  if [[ "$actual_status" != "$expected_status" ]]; then
    printf 'FAIL %s: expected exit %s, was %s\n%s\n' \
      "$label" "$expected_status" "$actual_status" "$output" >&2
    failures=$((failures + 1))
    return
  fi
  if [[ "$output" != *"$expected_text"* ]]; then
    printf 'FAIL %s: expected output to contain %s\n%s\n' \
      "$label" "$expected_text" "$output" >&2
    failures=$((failures + 1))
    return
  fi
  printf 'ok   %s\n' "$label"
}

# A workspace-wide version bump must not be treated as a cooldown event; those
# packages have no crates.io release and every lookup would fail.
setup_repo workspace-bump
printf 'serde 1.0.0 %s\n' "$old_date" > "$fixture"
write_lock "serde" "1.0.0" "0.2.0"
expect "workspace version bump is ignored" 0 "No new registry crate versions"

setup_repo old-crate
printf 'serde 1.1.0 %s\n' "$old_date" > "$fixture"
write_lock "serde" "1.1.0" "0.1.0"
expect "crate older than the window passes" 0 "are at least 3 days old"

setup_repo fresh-crate
printf 'serde 1.1.0 %s\n' "$fresh_date" > "$fixture"
write_lock "serde" "1.1.0" "0.1.0"
expect "fresh crate without an allow entry fails" 1 "within the 3-day cooldown"
if grep -Fq 'version = "1.1.0"' "${repo}/Cargo.lock"; then
  printf 'ok   default cooldown gate leaves the lock unchanged\n'
else
  printf 'FAIL default cooldown gate changed the lock\n' >&2
  failures=$((failures + 1))
fi

: > "$cargo_log"
expect "fix restores the prior version through Cargo" 0 \
  "serde 1.1.0 -> 1.0.0 (Cargo.lock)" --fix
if grep -Fq 'version = "1.0.0"' "${repo}/Cargo.lock" &&
  grep -Fq 'update --offline --manifest-path Cargo.toml -p serde@1.1.0 --precise 1.0.0' "$cargo_log"; then
  printf 'ok   fix uses an offline package-qualified precise Cargo update\n'
else
  printf 'FAIL fix did not restore the lock through the expected Cargo command\n' >&2
  failures=$((failures + 1))
fi

setup_repo ci-base
printf 'serde 1.1.0 %s\n' "$fresh_date" > "$fixture"
write_lock "serde" "1.1.0" "0.1.0"
git -C "$repo" add Cargo.lock
git -C "$repo" commit --quiet -m "update dependency"
status=0
output="$(CHANGED_BASE_SHA=HEAD^ run_check)" || status=$?
if [[ "$status" == 1 && "$output" == *"within the 3-day cooldown"* ]]; then
  printf 'ok   CI base detects a committed fresh crate\n'
else
  printf 'FAIL CI base handling: exit %s\n%s\n' "$status" "$output" >&2
  failures=$((failures + 1))
fi

status=0
output="$(CHANGED_BASE_SHA=0000000000000000000000000000000000000000 run_check)" || status=$?
if [[ "$status" == 1 && "$output" == *"within the 3-day cooldown"* ]]; then
  printf 'ok   CI branch-creation sentinel falls back to the parent commit\n'
else
  printf 'FAIL CI sentinel handling: exit %s\n%s\n' "$status" "$output" >&2
  failures=$((failures + 1))
fi

status=0
output="$(CHANGED_BASE_SHA=1111111111111111111111111111111111111111 run_check)" || status=$?
if [[ "$status" == 1 && "$output" == *"within the 3-day cooldown"* ]]; then
  printf 'ok   unreachable CI base falls back to the parent commit\n'
else
  printf 'FAIL unreachable CI base handling: exit %s\n%s\n' "$status" "$output" >&2
  failures=$((failures + 1))
fi

status=0
output="$(CHANGED_BASE_SHA=0000000000000000000000000000000000000000 \
  run_check --base HEAD)" || status=$?
if [[ "$status" == 0 && "$output" == *"No new registry crate versions vs HEAD"* ]]; then
  printf 'ok   explicit base overrides the CI comparison base\n'
else
  printf 'FAIL explicit base override: exit %s\n%s\n' "$status" "$output" >&2
  failures=$((failures + 1))
fi

setup_repo ci-remote-base
git -C "$repo" update-ref refs/remotes/origin/develop HEAD
printf 'serde 1.1.0 %s\n' "$fresh_date" > "$fixture"
write_lock "serde" "1.1.0" "0.1.0"
git -C "$repo" add Cargo.lock
git -C "$repo" commit --quiet -m "update dependency"
printf 'unrelated\n' > "${repo}/note.txt"
git -C "$repo" add note.txt
git -C "$repo" commit --quiet -m "add note"
status=0
output="$(CHANGED_BASE_SHA=0000000000000000000000000000000000000000 run_check)" || status=$?
if [[ "$status" == 1 && "$output" == *"within the 3-day cooldown"* ]]; then
  printf 'ok   CI sentinel uses the develop merge base across multiple commits\n'
else
  printf 'FAIL CI develop-base handling: exit %s\n%s\n' "$status" "$output" >&2
  failures=$((failures + 1))
fi

setup_repo offline-fallback
printf 'serde 1.1.0 %s\n' "$fresh_date" > "$fixture"
write_lock "serde" "1.1.0" "0.1.0"
: > "$cargo_log"
FAKE_CARGO_FAIL_OFFLINE_PACKAGE=serde@1.1.0 expect \
  "fix retries online when the offline pass cannot progress" 0 \
  "retrying 1 remaining rollback(s) with network access" --fix
if grep -Fq 'update --offline --manifest-path Cargo.toml -p serde@1.1.0 --precise 1.0.0' \
  "$cargo_log" &&
  grep -Fq 'update --manifest-path Cargo.toml -p serde@1.1.0 --precise 1.0.0' \
    "$cargo_log"; then
  printf 'ok   fix falls back to a network-capable precise Cargo update\n'
else
  printf 'FAIL fix did not retry the offline rollback with network access\n' >&2
  failures=$((failures + 1))
fi

# The update wrapper's snapshot is the rollback baseline for changes made by
# the current update, preserving older staged lockfile work.
setup_repo snapshot-prior
printf 'serde 1.0.5 %s\nserde 1.1.0 %s\n' "$old_date" "$fresh_date" > "$fixture"
write_lock "serde" "1.0.5" "0.1.0"
mkdir -p "${repo}/snapshot"
cp "${repo}/Cargo.lock" "${repo}/snapshot/Cargo.lock"
write_lock "serde" "1.1.0" "0.1.0"
: > "$cargo_log"
expect "fix prefers the pre-update snapshot version" 0 \
  "serde 1.1.0 -> 1.0.5 (Cargo.lock)" --fix --snapshot-dir snapshot

# A fresh violation already present in the snapshot still falls back to the
# committed prior version rather than accepting the violation unchanged.
setup_repo snapshot-existing-fresh
printf 'serde 1.0.5 %s\n' "$fresh_date" > "$fixture"
write_lock "serde" "1.0.5" "0.1.0"
mkdir -p "${repo}/snapshot"
cp "${repo}/Cargo.lock" "${repo}/snapshot/Cargo.lock"
: > "$cargo_log"
expect "fix repairs a fresh version already present in the snapshot" 0 \
  "serde 1.0.5 -> 1.0.0 (Cargo.lock)" --fix --snapshot-dir snapshot

# Snapshot-relative candidates remain visible even when the update happens to
# return the lockfile to the exact version already committed at HEAD.
setup_repo snapshot-only-change
printf 'serde 1.0.0 %s\nserde 1.1.0 %s\n' "$old_date" "$fresh_date" > "$fixture"
write_lock "serde" "1.1.0" "0.1.0"
git -C "$repo" add Cargo.lock
git -C "$repo" commit --quiet --amend --no-edit
write_lock "serde" "1.0.0" "0.1.0"
mkdir -p "${repo}/snapshot"
cp "${repo}/Cargo.lock" "${repo}/snapshot/Cargo.lock"
write_lock "serde" "1.1.0" "0.1.0"
: > "$cargo_log"
expect "fix detects an update hidden by the HEAD comparison" 0 \
  "serde 1.1.0 -> 1.0.0 (Cargo.lock)" --fix --snapshot-dir snapshot

setup_repo fresh-allowed-no-audit
printf 'serde 1.1.0 %s\n' "$fresh_date" > "$fixture"
write_cargo_toml 'serde = "1.1.0"'
write_lock "serde" "1.1.0" "0.1.0"
expect "allowed crate without an audit fails" 1 "missing a cargo-vet audit"

setup_repo fresh-allowed-audited
printf 'serde 1.1.0 %s\n' "$fresh_date" > "$fixture"
write_cargo_toml 'serde = "1.1.0"'
write_audits '[[audits.serde]]
who = "Test <test@example.com>"
criteria = "safe-to-deploy"
delta = "1.0.0 -> 1.1.0"'
write_lock "serde" "1.1.0" "0.1.0"
expect "allowed crate with a delta audit passes" 0 "audited exception"

: > "$cargo_log"
expect "fix preserves an audited allow entry" 0 "No cooldown rollback required" --fix
if grep -Fq 'version = "1.1.0"' "${repo}/Cargo.lock" &&
  ! grep -q '^update ' "$cargo_log"; then
  printf 'ok   audited allow entry bypasses rollback\n'
else
  printf 'FAIL audited allow entry was unexpectedly rolled back\n' >&2
  failures=$((failures + 1))
fi

setup_repo fresh-allowed-full-audit
printf 'serde 1.1.0 %s\n' "$fresh_date" > "$fixture"
write_cargo_toml 'serde = "1.1.0"'
write_audits '[[audits.serde]]
who = "Test <test@example.com>"
criteria = "safe-to-deploy"
version = "1.1.0"'
write_lock "serde" "1.1.0" "0.1.0"
expect "allowed crate with a full-version audit passes" 0 "audited exception"

# An allow entry pinned to a different version must not carry the new one.
setup_repo allow-version-mismatch
printf 'serde 1.1.0 %s\n' "$fresh_date" > "$fixture"
write_cargo_toml 'serde = "1.0.9"'
write_audits '[[audits.serde]]
who = "Test <test@example.com>"
criteria = "safe-to-deploy"
delta = "1.0.0 -> 1.1.0"'
write_lock "serde" "1.1.0" "0.1.0"
expect "allow entry for another version does not apply" 1 "within the 3-day cooldown"

# A newly introduced transitive crate has no safe prior version to select
setup_repo new-transitive
printf 'serde 1.0.0 %s\nanyhow 2.0.0 %s\n' "$old_date" "$fresh_date" > "$fixture"
cat >> "${repo}/Cargo.lock" << 'LOCK'

[[package]]
name = "anyhow"
version = "2.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "1111111111111111111111111111111111111111111111111111111111111111"
LOCK
: > "$cargo_log"
expect "fix refuses a fresh crate without a prior version" 1 \
  "no prior version is present in the diff" --fix
if [[ ! -s "$cargo_log" ]]; then
  printf 'ok   ambiguous repair does not invoke Cargo\n'
else
  printf 'FAIL ambiguous repair invoked Cargo\n' >&2
  failures=$((failures + 1))
fi

setup_repo multiple-fresh-versions
printf 'serde 1.1.0 %s\nserde 1.2.0 %s\n' "$fresh_date" "$fresh_date" > "$fixture"
write_lock "serde" "1.1.0" "0.1.0"
cat >> "${repo}/Cargo.lock" << 'LOCK'

[[package]]
name = "serde"
version = "1.2.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "2222222222222222222222222222222222222222222222222222222222222222"
LOCK
: > "$cargo_log"
expect "fix refuses multiple fresh versions without unique pairing" 1 \
  "2 fresh versions have no unique rollback pairing" --fix
if [[ ! -s "$cargo_log" ]]; then
  printf 'ok   multi-version ambiguity does not invoke Cargo\n'
else
  printf 'FAIL multi-version ambiguity invoked Cargo\n' >&2
  failures=$((failures + 1))
fi

setup_repo cargo-failure
printf 'serde 1.1.0 %s\n' "$fresh_date" > "$fixture"
write_lock "serde" "1.1.0" "0.1.0"
FAKE_CARGO_FAIL_PACKAGE=serde@1.1.0 expect "Cargo rollback failure remains fatal" 1 \
  "could not make progress on 1 cooldown rollback" --fix

# An unreachable registry must never pass silently.
setup_repo lookup-failure
: > "$fixture"
write_lock "serde" "1.1.0" "0.1.0"
expect "unreachable registry fails closed" 1 "could not be reached on crates.io"

# The gate must work where GNU date is unavailable, as on macOS.
setup_repo bsd-date
printf 'serde 1.1.0 %s\n' "$fresh_date" > "$fixture"
write_lock "serde" "1.1.0" "0.1.0"
USE_BSD_DATE=1 expect "BSD style date still evaluates the window" 1 "within the 3-day cooldown"

# A crate version present in two locks is reported once, not per lock.
setup_repo multi-lock
printf 'serde 1.1.0 %s\n' "$old_date" > "$fixture"
write_lock "serde" "1.1.0" "0.1.0"
cp "${repo}/Cargo.lock" "${repo}/Fuzz.lock"
git -C "$repo" add Fuzz.lock
git -C "$repo" commit --quiet -m "add second lock"
write_lock "serde" "1.2.0" "0.1.0"
cp "${repo}/Cargo.lock" "${repo}/Fuzz.lock"
printf 'serde 1.2.0 %s\n' "$old_date" > "$fixture"
status=0
output="$(run_check --lock Cargo.lock --lock Fuzz.lock)" || status=$?
if [[ "$status" == 0 && "$(grep -c '^serde ' <<< "$output")" == 1 ]]; then
  printf 'ok   crate in two locks is checked once\n'
else
  printf 'FAIL crate in two locks: exit %s\n%s\n' "$status" "$output" >&2
  failures=$((failures + 1))
fi

# Fix mode maps each lock to the Cargo.toml beside it, including standalone
# fuzz workspaces outside the root workspace.
setup_repo multi-lock-fix
mkdir -p "${repo}/nested"
printf '[workspace]\nmembers = []\n' > "${repo}/nested/Cargo.toml"
cp "${repo}/Cargo.lock" "${repo}/nested/Cargo.lock"
git -C "$repo" add nested
git -C "$repo" commit --quiet -m "add standalone workspace"
printf 'serde 1.1.0 %s\n' "$fresh_date" > "$fixture"
write_lock "serde" "1.1.0" "0.1.0"
cp "${repo}/Cargo.lock" "${repo}/nested/Cargo.lock"
: > "$cargo_log"
status=0
output="$(run_check --fix --lock Cargo.lock --lock nested/Cargo.lock)" || status=$?
if [[ "$status" == 0 && "$output" == *"Rolled back 2 fresh lockfile update(s)"* ]] &&
  grep -Fq -- '--manifest-path Cargo.toml' "$cargo_log" &&
  grep -Fq -- '--manifest-path nested/Cargo.toml' "$cargo_log"; then
  printf 'ok   fix updates root and standalone workspace locks\n'
else
  printf 'FAIL multi-lock fix: exit %s\n%s\n' "$status" "$output" >&2
  failures=$((failures + 1))
fi

setup_repo bad-argument
status=0
output="$(run_check --nonsense)" || status=$?
if [[ "$status" == 2 && "$output" == *"Unknown argument"* ]]; then
  printf 'ok   unknown argument exits 2\n'
else
  printf 'FAIL unknown argument: exit %s\n%s\n' "$status" "$output" >&2
  failures=$((failures + 1))
fi

setup_repo help
status=0
output="$(run_check --help)" || status=$?
if [[ "$status" == 0 &&
  "$output" == *"use \`SKIP=cargo-cooldown\` to commit"* &&
  "$output" == *"without the gate."* ]]; then
  printf 'ok   help includes the complete policy header\n'
else
  printf 'FAIL help output: exit %s\n%s\n' "$status" "$output" >&2
  failures=$((failures + 1))
fi

if ((failures > 0)); then
  printf '\n%s check-cargo-cooldown test(s) failed\n' "$failures" >&2
  exit 1
fi

printf '\nAll check-cargo-cooldown tests passed\n'
