#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

test_root="$(mktemp -d "${TMPDIR:-/tmp}/nautilus-cooldown-consumer.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT

git -C "$REPO_ROOT" ls-files '*Cargo.lock' > "${test_root}/actual-locks"
printf '%s\n' \
  'Cargo.lock' \
  'crates/adapters/lighter/fuzz/pornin/Cargo.lock' \
  > "${test_root}/expected-locks"

if ! cmp -s "${test_root}/expected-locks" "${test_root}/actual-locks"; then
  echo "Tracked Cargo lockfiles do not match the NautilusTrader consumer layout" >&2
  diff -u "${test_root}/expected-locks" "${test_root}/actual-locks" >&2 || true
  exit 1
fi

fixture_repo="${test_root}/repo"
fake_bin="${test_root}/bin"
fuzz_path="crates/adapters/lighter/fuzz/pornin"
mkdir -p "${fixture_repo}/scripts" "${fixture_repo}/${fuzz_path}" "$fake_bin"
cp "${REPO_ROOT}/scripts/check-cargo-cooldown.sh" "${fixture_repo}/scripts/"

printf '%s\n' \
  '[workspace]' \
  'members = []' \
  '' \
  '[workspace.metadata.cooldown]' \
  'days = 3' \
  > "${fixture_repo}/Cargo.toml"
printf '%s\n' '[workspace]' 'members = []' > "${fixture_repo}/${fuzz_path}/Cargo.toml"
printf '%s\n' 'version = 4' > "${fixture_repo}/Cargo.lock"
cp "${fixture_repo}/Cargo.lock" "${fixture_repo}/${fuzz_path}/Cargo.lock"

cat > "${fake_bin}/curl" << 'FAKE_CURL'
#!/usr/bin/env bash
echo "Cargo cooldown consumer check unexpectedly accessed the network" >&2
exit 1
FAKE_CURL
chmod +x "${fake_bin}/curl"

git -C "$fixture_repo" init --quiet
git -C "$fixture_repo" config user.email test@example.com
git -C "$fixture_repo" config user.name Test
git -C "$fixture_repo" config commit.gpgsign false
git -C "$fixture_repo" add -A
git -C "$fixture_repo" commit --quiet -m baseline

output=$(cd "$fixture_repo" &&
  PATH="${fake_bin}:${PATH}" bash scripts/check-cargo-cooldown.sh --base HEAD)
if [[ "$output" != "No new registry crate versions vs HEAD." ]]; then
  printf 'Unexpected Cargo cooldown result: %s\n' "$output" >&2
  exit 1
fi

echo "Cargo cooldown consumer check passed"
