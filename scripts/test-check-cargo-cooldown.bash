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
cp "${COOLDOWN_SCRIPT_SOURCE:-${REPO_ROOT}/scripts/check-cargo-cooldown.sh}" "${fixture_repo}/scripts/"

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
printf '%s\n' "$*" >> "${COOLDOWN_NETWORK_LOG:?}"
echo "Cargo cooldown consumer check unexpectedly accessed the network" >&2
exit 1
FAKE_CURL
chmod +x "${fake_bin}/curl"
export COOLDOWN_NETWORK_LOG="${test_root}/network.log"

git -C "$fixture_repo" init --quiet
git -C "$fixture_repo" config user.email test@example.com
git -C "$fixture_repo" config user.name Test
git -C "$fixture_repo" config commit.gpgsign false
git -C "$fixture_repo" add -A
git -C "$fixture_repo" commit --quiet -m baseline

if [[ ! -f "$REPO_ROOT/.supply-chain/crate-dates.json" ]]; then
  echo "Cooldown database is missing" >&2
  exit 1
fi
if git -C "$REPO_ROOT" check-ignore -q .supply-chain/crate-dates.json; then
  echo "Cooldown database is ignored" >&2
  exit 1
fi
if ! grep -Fq 'crate-dates' "$REPO_ROOT/.pre-commit-config.yaml"; then
  echo "cargo-cooldown hook does not watch the publication-date database" >&2
  exit 1
fi

command -v python3 > /dev/null || {
  echo "Required test command not on PATH: python3" >&2
  exit 1
}

db_count=$(
  python3 - "$REPO_ROOT" "${test_root}/actual-locks" << 'PY'
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
lock_list = pathlib.Path(sys.argv[2])
db = json.loads((root / ".supply-chain/crate-dates.json").read_text())
keys = set((db.get("entries") or {}).keys())
lock_keys = set()
for rel in lock_list.read_text().splitlines():
    name = ver = src = None
    for line in (root / rel).read_text().splitlines():
        if line.startswith("[[package]]"):
            if name and ver and src and "crates.io" in src:
                lock_keys.add(f"{name}@{ver}")
            name = ver = src = None
            continue
        if line.startswith('name = "') and line.endswith('"'):
            name = line[len('name = "') : -1]
        elif line.startswith('version = "') and line.endswith('"'):
            ver = line[len('version = "') : -1]
        elif line.startswith('source = "') and line.endswith('"'):
            src = line[len('source = "') : -1]
    if name and ver and src and "crates.io" in src:
        lock_keys.add(f"{name}@{ver}")
missing = sorted(lock_keys - keys)
extra = sorted(keys - lock_keys)
if missing or extra:
    sys.stderr.write(
        f"Cooldown database does not match tracked registry versions "
        f"(missing {len(missing)}, extra {len(extra)})\n"
    )
    sys.exit(1)
print(len(lock_keys))
PY
)

status=0
output=$(cd "$REPO_ROOT" &&
  PATH="${fake_bin}:${PATH}" bash scripts/check-cargo-cooldown.sh --all) || status=$?
if ((status != 0)) ||
  [[ "$output" != *"Publication dates: ${db_count} from the cooldown database, 0 from crates.io"* ]]; then
  printf 'Offline full cooldown check did not use the committed database: %s\n' "$output" >&2
  exit 1
fi

output=$(cd "$fixture_repo" &&
  PATH="${fake_bin}:${PATH}" bash scripts/check-cargo-cooldown.sh --all)
if [[ "$output" != "No resolved registry crate versions"* ]]; then
  printf 'Unexpected Cargo cooldown result: %s\n' "$output" >&2
  exit 1
fi

hook_entry=$(awk '
  $0 ~ /- id: cargo-cooldown$/ { in_hook = 1; next }
  in_hook && $1 == "entry:" { sub(/^[[:space:]]*entry: /, ""); print; exit }
' "$REPO_ROOT/.pre-commit-config.yaml")
read -r -a hook_command <<< "$hook_entry"
[[ ${#hook_command[@]} -gt 0 ]] || exit 1

mkdir -p "${fixture_repo}/.supply-chain"
cat >> "${fixture_repo}/Cargo.lock" << 'LOCK'

[[package]]
name = "cooldown-fixture"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
LOCK
cat > "${fixture_repo}/.supply-chain/crate-dates.json" << 'DATABASE'
{"schema":1,"entries":{"cooldown-fixture@1.0.0":{"published":"2020-01-01T00:00:00Z","verified_at":"2020-01-01T00:00:00Z"}}}
DATABASE
git -C "$fixture_repo" add Cargo.lock .supply-chain/crate-dates.json

status=0
output=$(cd "$fixture_repo" && PATH="${fake_bin}:${PATH}" \
  "${hook_command[@]}") || status=$?
if ((status != 0)) || [[ -s "$COOLDOWN_NETWORK_LOG" ]] ||
  [[ "$output" != *"Publication dates: 1 from the cooldown database, 0 from crates.io"* ]]; then
  printf 'Cooldown hook re-fetched a date added since the base: %s\n' "$output" >&2
  exit 1
fi

python3 - "${fixture_repo}/.supply-chain/crate-dates.json" << 'PY'
import datetime
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
data = json.loads(path.read_text())
data["entries"]["cooldown-fixture@1.0.0"]["published"] = (
    datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
)
path.write_text(json.dumps(data) + "\n")
PY
status=0
output=$(cd "$fixture_repo" && PATH="${fake_bin}:${PATH}" \
  "${hook_command[@]}") || status=$?
if ((status != 1)) || [[ -s "$COOLDOWN_NETWORK_LOG" ]] ||
  [[ "$output" != *"FAIL: 1 crate(s) within the 3-day cooldown"* ]]; then
  printf 'Cooldown hook did not reject a fresh recorded crate offline: %s\n' "$output" >&2
  exit 1
fi

build_log="${test_root}/build.log"
build_makefile="${test_root}/build.mk"
build_target="${test_root}/target"
cat > "$build_makefile" << 'BUILD_MAKEFILE'
.PHONY: check-cargo-cooldown
check-cargo-cooldown:
	@printf '%s\n' cooldown >> "$(BUILD_LOG)"
	@exit $(COOLDOWN_STATUS)
BUILD_MAKEFILE

cat > "${fake_bin}/uv" << 'FAKE_UV'
#!/usr/bin/env bash
case "$*" in
  --version) echo 'uv 0.12.3' ;;
  *generate_stubs.py*|*maturin*) printf '%s\n' "$*" >> "${BUILD_LOG:?}" ;;
esac
FAKE_UV
cat > "${fake_bin}/cargo" << 'FAKE_CARGO'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "${BUILD_LOG:?}"
FAKE_CARGO
chmod +x "${fake_bin}/uv" "${fake_bin}/cargo"

run_build() {
  PATH="${fake_bin}:${PATH}" BUILD_LOG="$build_log" \
    make -C "$REPO_ROOT" --no-print-directory -j2 \
    -f Makefile -f "$build_makefile" \
    CARGO_CI_PROFILE=nextest \
    TARGET_DIR="$build_target" PY_STUB_INPUTS= PY_STUB_INPUT_LIST_COMMAND=true \
    "$@" > "${test_root}/make.log" 2>&1
}

if awk '
  /^[^#[:space:]][^:]*:/ && !/^check-cargo-cooldown:/ &&
    /:.*check-cargo-cooldown/ { found = 1 }
  END { exit !found }
' "$REPO_ROOT/Makefile"; then
  echo "Routine Make targets still require the full cooldown check" >&2
  exit 1
fi

for source in .github/actions/common-setup/action.yml .github/workflows/docker.yml; do
  if grep -Fq check-cargo-cooldown "$REPO_ROOT/$source"; then
    echo "Build setup still invokes the cooldown check: $source" >&2
    exit 1
  fi
done

: > "$build_log"
run_build COOLDOWN_STATUS=37 build-wheel
if grep -Fxq cooldown "$build_log" ||
  ! grep -Fq 'maturin build --release --locked' "$build_log"; then
  echo "Wheel build did not run independently of the cooldown check" >&2
  exit 1
fi

: > "$build_log"
run_build COOLDOWN_STATUS=37 py-stubs
if grep -Fxq cooldown "$build_log" ||
  ! grep -Fq generate_stubs.py "$build_log" || [[ ! -f "$build_target/.py-stubs.stamp" ]]; then
  echo "Stub generation did not run independently of the cooldown check" >&2
  exit 1
fi

for cargo_target in "" "${test_root}/persistent-target"; do
  expected_target=${cargo_target:-$build_target}
  command=$(make -C "$REPO_ROOT" --no-print-directory --dry-run \
    TARGET_DIR="$build_target" CARGO_TARGET_DIR="$cargo_target" check-cargo-cooldown)
  expected="bash scripts/check-cargo-cooldown.sh --all --cache \"${expected_target}/.cargo-cooldown.json\""
  if [[ "$command" != "$expected" ]]; then
    printf 'Unexpected cooldown invocation: %s\n' "$command" >&2
    exit 1
  fi
done

for source in \
  Makefile \
  scripts/clippy-changed.sh \
  scripts/doc-changed.sh \
  scripts/ci/test-postgres-bootstrap.bash \
  .pre-commit-hooks/cargo_clippy_network_turmoil_non_linux.sh \
  .github/workflows/build.yml \
  .github/workflows/test.yml \
  .github/workflows/cli-binaries.yml \
  .github/workflows/nightly-tests.yml \
  .docker/nautilus_trader.dockerfile \
  scripts/regen-capnp.sh; do
  if ! awk '
    BEGIN {
      build = "(build|check|test|run|clippy|doc|bench|nextest[[:space:]]+run)"
      wrapper = "(miri[[:space:]]+)?" build
      wrapper = wrapper "|llvm-cov[[:space:]]+nextest|codspeed[[:space:]]+build"
      wrapper = wrapper "|hack[[:space:]].*[[:space:]](check|doc)"
      compilation = "cargo([[:space:]]+\\+[^[:space:]]+)?[[:space:]]+(" wrapper ")"
      compilation = "(" compilation "|maturin[[:space:]]+(build|develop))[[:space:]]"
    }
    FILENAME ~ /\/Makefile$/ && $0 !~ /^\t/ { next }
    /^[[:space:]]*#/ { next }
    {
      continued = sub(/\\$/, "")
      command = command $0 " "
      if (continued) next
      if (command ~ compilation &&
          command !~ /--locked([[:space:]]|$)/) {
        print FILENAME ": unlocked compilation: " command
        failed = 1
      }
      command = ""
    }
    END { exit failed }
  ' "$REPO_ROOT/$source"; then
    exit 1
  fi
done

echo "Cargo cooldown consumer check passed"
