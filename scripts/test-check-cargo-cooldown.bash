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

cp "${fake_bin}/curl" "${curl_only_bin}/curl"

fixture="${test_root}/crates.txt"
export FAKE_CRATES_FIXTURE="$fixture"
export REAL_DATE

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
  local actual_status=0
  local output=""
  # Keep the `||` outside the substitution so the status lands in this scope.
  output="$(run_check)" || actual_status=$?
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

setup_repo bad-argument
status=0
output="$(run_check --nonsense)" || status=$?
if [[ "$status" == 2 && "$output" == *"Unknown argument"* ]]; then
  printf 'ok   unknown argument exits 2\n'
else
  printf 'FAIL unknown argument: exit %s\n%s\n' "$status" "$output" >&2
  failures=$((failures + 1))
fi

if ((failures > 0)); then
  printf '\n%s check-cargo-cooldown test(s) failed\n' "$failures" >&2
  exit 1
fi

printf '\nAll check-cargo-cooldown tests passed\n'
