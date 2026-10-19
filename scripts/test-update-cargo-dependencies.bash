#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
UPDATE_SCRIPT="${SCRIPT_DIR}/update-cargo-dependencies.bash"
CHECK_SCRIPT="${SCRIPT_DIR}/check-cargo-cooldown.sh"

for required in cmp git grep; do
  command -v "$required" > /dev/null || {
    echo "Required test command not on PATH: $required" >&2
    exit 1
  }
done

cargo_update_recipe=$(sed -n '/^cargo-update:/,/^$/p' "${REPO_ROOT}/Makefile")
if [[ "$cargo_update_recipe" != *"bash scripts/update-cargo-dependencies.bash"* ]]; then
  echo "Make cargo-update does not use the shared transaction script" >&2
  exit 1
fi

test_root="$(mktemp -d "${TMPDIR:-/tmp}/nautilus-cargo-update-consumer.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT

repo="${test_root}/repo"
fake_bin="${test_root}/bin"
cargo_log="${test_root}/cargo.log"
fuzz_path="crates/adapters/lighter/fuzz/pornin"
mkdir -p "${repo}/scripts" "${repo}/${fuzz_path}" "$fake_bin"
cp "$UPDATE_SCRIPT" "$CHECK_SCRIPT" "${repo}/scripts/"

printf '%s\n' \
  '[workspace]' \
  'members = []' \
  '' \
  '[workspace.metadata.cooldown]' \
  'days = 3' \
  > "${repo}/Cargo.toml"
printf '%s\n' '[workspace]' 'members = []' > "${repo}/${fuzz_path}/Cargo.toml"
printf '%s\n' 'version = 4' > "${repo}/Cargo.lock"
cp "${repo}/Cargo.lock" "${repo}/${fuzz_path}/Cargo.lock"

cat > "${fake_bin}/cargo" << 'FAKE_CARGO'
#!/usr/bin/env bash
set -euo pipefail

printf '%s\n' "$*" >> "${FAKE_CARGO_LOG:?}"
case "${1:-}" in
  update | metadata) ;;
  *)
    printf 'Unexpected Cargo command: %s\n' "$*" >&2
    exit 2
    ;;
esac
FAKE_CARGO
chmod +x "${fake_bin}/cargo"

git -C "$repo" init --quiet
git -C "$repo" config user.email test@example.com
git -C "$repo" config user.name Test
git -C "$repo" config commit.gpgsign false
git -C "$repo" add -A
git -C "$repo" commit --quiet -m baseline
cp "${repo}/Cargo.lock" "${test_root}/expected-root.lock"
cp "${repo}/${fuzz_path}/Cargo.lock" "${test_root}/expected-fuzz.lock"

output=$(cd "$test_root" &&
  FAKE_CARGO_LOG="$cargo_log" PATH="${fake_bin}:${PATH}" \
    bash "${repo}/scripts/update-cargo-dependencies.bash")

if [[ "$output" != *"Cargo dependency update complete; cooldown policy enforced"* ]]; then
  printf 'Cargo update did not complete: %s\n' "$output" >&2
  exit 1
fi
if [[ $(wc -l < "$cargo_log") -ne 4 ]] ||
  ! grep -Fxq 'update' "$cargo_log" ||
  ! grep -Fxq "update --manifest-path ${fuzz_path}/Cargo.toml" "$cargo_log" ||
  ! grep -Fxq 'metadata --manifest-path Cargo.toml --locked --format-version 1' "$cargo_log" ||
  ! grep -Fxq \
    "metadata --manifest-path ${fuzz_path}/Cargo.toml --locked --format-version 1" \
    "$cargo_log"; then
  echo "Cargo update did not discover both NautilusTrader workspaces" >&2
  cat "$cargo_log" >&2
  exit 1
fi
if ! cmp -s "${test_root}/expected-root.lock" "${repo}/Cargo.lock" ||
  ! cmp -s "${test_root}/expected-fuzz.lock" "${repo}/${fuzz_path}/Cargo.lock"; then
  echo "Cargo update changed a lockfile in the no-change consumer fixture" >&2
  exit 1
fi

transaction_lock=$(git -C "$repo" rev-parse --git-path nautilus-cargo-update.lock)
if [[ -e "${repo}/${transaction_lock}" ]]; then
  echo "Cargo update left its transaction lock behind" >&2
  exit 1
fi

echo "Cargo dependency update consumer check passed"
