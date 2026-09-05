#!/usr/bin/env bash
# Mock script variables expand when the generated files run.
# shellcheck disable=SC2016

set -euo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)
CASE_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/nautilus-artifact-reuse.XXXXXX")
trap 'rm -rf "$CASE_ROOT"' EXIT

MOCK_BIN="$CASE_ROOT/bin"
UV_LOG="$CASE_ROOT/uv.log"
CARGO_LOG="$CASE_ROOT/cargo.log"
GIT_LOG="$CASE_ROOT/git.log"
MAKE_LOG="$CASE_ROOT/make.log"
RUST_CHECK_LOG="$CASE_ROOT/rust-check.log"
TARGET_DIR="$CASE_ROOT/target"
SOURCE_DIR="$CASE_ROOT/source"
MAKE_BIN=$(command -v make)

mkdir -p "$MOCK_BIN" "$SOURCE_DIR"
: > "$SOURCE_DIR/input.rs"
: > "$UV_LOG"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  '' \
  'set -euo pipefail' \
  '' \
  'if [ "${1:-}" = "--version" ]; then' \
  '  echo "uv 0.12.3"' \
  '  exit 0' \
  'fi' \
  '' \
  'printf "%s|stub-profile=%s\n" "$*" "${NAUTILUS_STUB_PROFILE:-}" >> "${UV_LOG:?}"' > "$MOCK_BIN/uv"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  '' \
  'set -euo pipefail' \
  '' \
  'printf "%s\n" "$*" >> "${CARGO_LOG:?}"' > "$MOCK_BIN/cargo"

chmod +x "$MOCK_BIN/uv" "$MOCK_BIN/cargo"

fail() {
  echo "ERROR: $1" >&2
  exit 1
}

run_make() {
  local inputs="$1"
  local input_list_command="printf '%s\\n' $inputs"
  shift

  PATH="$MOCK_BIN:$PATH" \
    UV_LOG="$UV_LOG" \
    make -C "$REPO_ROOT" --no-print-directory \
    TARGET_DIR="$TARGET_DIR" \
    CARGO_CI_PROFILE=nextest \
    PY_STUB_INPUTS="$inputs" \
    PY_STUB_INPUT_LIST_COMMAND="$input_list_command" \
    PYTHON_EXTENSION_PATH="$SOURCE_DIR/_libnautilus.so" \
    "$@" > /dev/null
}

stub_generation_count() {
  grep -Fc "run --no-sync python generate_stubs.py" "$UV_LOG" || true
}

source_inputs="$SOURCE_DIR $SOURCE_DIR/input.rs"
run_make "$source_inputs" py-stubs
[[ -f "$TARGET_DIR/.py-stubs.stamp" ]] || fail "Stub generation did not create its stamp"
[[ "$(stub_generation_count)" -eq 1 ]] || fail "Initial stub generation did not run once"
grep -Fq "run --no-sync python generate_stubs.py|stub-profile=nextest" "$UV_LOG" ||
  fail "Stub generation did not use the nextest profile"

run_make "$source_inputs" py-stubs
[[ "$(stub_generation_count)" -eq 1 ]] || fail "Fresh stub inputs triggered regeneration"

sleep 1
touch "$SOURCE_DIR/input.rs"
run_make "$source_inputs" py-stubs
[[ "$(stub_generation_count)" -eq 2 ]] || fail "A changed stub input did not trigger regeneration"

sleep 1
rm "$SOURCE_DIR/input.rs"
run_make "" py-stubs
[[ "$(stub_generation_count)" -eq 3 ]] || fail "A deleted stub input did not trigger regeneration"

run_make "$SOURCE_DIR" pytest-collect-fast
grep -Fq "run --no-sync pytest tests/ --collect-only -q" "$UV_LOG" ||
  fail "Fast Python collection did not collect the test tree"
if grep -Fq "maturin develop" "$UV_LOG"; then
  fail "Fast Python collection rebuilt the extension"
fi

collection_count=$(grep -Fc "run --no-sync pytest tests/ --collect-only -q" "$UV_LOG")
run_make "$SOURCE_DIR" PYTHON_EXTENSION_PATH= pytest-collect-fast
[[ "$(grep -Fc "run --no-sync pytest tests/ --collect-only -q" "$UV_LOG")" -eq "$collection_count" ]] ||
  fail "Fast Python collection ran without an existing extension"

run_make "$SOURCE_DIR" pytest
grep -Fq "run --no-sync maturin develop --profile nextest" "$UV_LOG" ||
  fail "Full Python tests did not build with the nextest profile"

grep -Fq -- "- id: python-test-collection" "$REPO_ROOT/.pre-commit-config.yaml" ||
  fail "Python collection is not registered with pre-commit"
grep -Fq "entry: make pytest-collect-fast" "$REPO_ROOT/.pre-commit-config.yaml" ||
  fail "Pre-commit does not use fast Python collection"

pre_flight_recipe=$(sed -n '/^pre-flight:  /,/^\t\$(call timer_end,Pre-flight)/p' "$REPO_ROOT/Makefile")
[[ "$pre_flight_recipe" == *'check-code-sim'*'cargo-test-sim'*'cargo-test-extras'* ]] ||
  fail "Pre-flight Rust checks do not preserve early DST linting and test order"
[[ "$pre_flight_recipe" != *'cargo-test-doc'* ]] ||
  fail "Pre-flight still runs Rust doctests"

for workflow in build.yml test.yml; do
  if grep -Fq 'make cargo-test-doc' "$REPO_ROOT/.github/workflows/$workflow"; then
    fail "Regular CI still runs Rust doctests: $workflow"
  fi
done

nightly_doctest_job=$(awk '
  /^  rust-doctests:/ {
    capture = 1
  }
  capture && /^  [[:alnum:]_-]+:/ && !/^  rust-doctests:/ {
    exit
  }
  capture {
    print
  }
' "$REPO_ROOT/.github/workflows/nightly-tests.yml")
[[ -n "$nightly_doctest_job" ]] || fail "Nightly tests do not define a Rust doctest job"
[[ "$nightly_doctest_job" == *$'        python-version:\n          - "3.13"\n          - "3.14"'* ]] ||
  fail "Nightly Rust doctests do not cover Python 3.13 and 3.14"
[[ "$nightly_doctest_job" == *'make cargo-test-doc'* ]] ||
  fail "Nightly tests do not run Rust doctests"
[[ "$nightly_doctest_job" == *'EXTRA_FEATURES="capnp,hypersync,nautilus-serialization/sbe,nautilus-infrastructure/postgres"'* ]] ||
  fail "Nightly Rust doctests do not preserve the CI feature set"

: > "$CARGO_LOG"
PATH="$MOCK_BIN:$PATH" \
  CARGO_LOG="$CARGO_LOG" \
  "$MAKE_BIN" -C "$REPO_ROOT" --no-print-directory \
  CARGO_CI_PROFILE=nextest \
  NEXTEST_PROFILE=ci \
  cargo-test-sim > /dev/null
[[ "$(grep -Fc 'nextest run ' "$CARGO_LOG")" -eq 2 ]] ||
  fail "DST smoke tests did not use two feature-coherent nextest runs"
if grep -Eq '^build ' "$CARGO_LOG"; then
  fail "DST smoke tests used a redundant Cargo build"
fi
grep -Fq \
  'nextest run --config target."cfg(all())".rustflags=["--cfg","madsim"] -p nautilus-common -p nautilus-core -p nautilus-event-store -p nautilus-network -p nautilus-execution -p nautilus-live --lib --tests --features simulation' \
  "$CARGO_LOG" || fail "Standard-precision DST tests did not compile the full package scope together"
grep -Fq \
  'nextest run --config target."cfg(all())".rustflags=["--cfg","madsim"] -p nautilus-common -p nautilus-execution --lib --tests --features simulation,high-precision' \
  "$CARGO_LOG" || fail "High-precision DST tests did not share one feature-coherent build"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  '' \
  'set -euo pipefail' \
  '' \
  'if [ "${1:-}" = "diff" ]; then' \
  '  if [ -n "${GIT_CHANGED_FILES:-}" ]; then' \
  '    printf "%s\n" "$GIT_CHANGED_FILES"' \
  '  fi' \
  '  exit 0' \
  'fi' \
  '' \
  'echo "Unexpected git command: $*" >&2' \
  'exit 1' > "$MOCK_BIN/git"
chmod +x "$MOCK_BIN/git"

run_changed_script() {
  local script="$1"
  local changed_files="$2"

  : > "$CARGO_LOG"
  : > "$RUST_CHECK_LOG"
  PATH="$MOCK_BIN:$PATH" \
    CARGO_CI_PROFILE=nextest \
    CARGO_LOG="$CARGO_LOG" \
    GIT_CHANGED_FILES="$changed_files" \
    CHANGED_BASE_SHA='' \
    bash "$REPO_ROOT/scripts/$script" > "$RUST_CHECK_LOG"
}

# Both hooks and the Makefile gates read one feature definition, so assert against
# it rather than a copy that can drift out from under them.
BASE_FEATURES=$(bash "$REPO_ROOT/scripts/cargo-features.bash")

run_changed_script clippy-changed.sh ""
grep -Fq \
  "clippy --workspace --lib --bins --tests --features $BASE_FEATURES --profile nextest -- -D warnings" \
  "$CARGO_LOG" || fail "Clippy features do not match the shared feature set"

run_changed_script doc-changed.sh ""
grep -Fq \
  "doc --workspace --no-deps --quiet --features $BASE_FEATURES --profile nextest" \
  "$CARGO_LOG" || fail "Cargo doc clean-checkout fallback did not cover the workspace"

# The shared feature definition changes every crate's feature graph, so a change
# to it alone must force a full workspace run rather than be filtered out.
run_changed_script clippy-changed.sh "scripts/cargo-features.bash"
grep -Fq \
  "clippy --workspace --lib --bins --tests --features $BASE_FEATURES --profile nextest -- -D warnings" \
  "$CARGO_LOG" || fail "Shared feature change did not force a full workspace Clippy run"

run_changed_script doc-changed.sh "scripts/cargo-features.bash"
grep -Fq \
  "doc --workspace --no-deps --quiet --features $BASE_FEATURES --profile nextest" \
  "$CARGO_LOG" || fail "Shared feature change did not force a full workspace doc run"

# The harness mock git discards pathspecs and pins CHANGED_BASE_SHA empty, so it
# cannot reach the "$base"..HEAD pathspec that decides the CI path. Drive that
# branch against a real repository with only cargo mocked.
CARGO_ONLY_BIN="$CASE_ROOT/cargo-only-bin"
mkdir -p "$CARGO_ONLY_BIN"
cp "$MOCK_BIN/cargo" "$CARGO_ONLY_BIN/cargo"

real_git_repo() {
  local repo="$CASE_ROOT/$1"

  mkdir -p "$repo/scripts" "$repo/crates/core/src"
  git -c init.defaultBranch=develop init -q "$repo"
  git -C "$repo" config user.email "script-tests@example.com"
  git -C "$repo" config user.name "Script Tests"
  git -C "$repo" config commit.gpgsign false
  cp "$REPO_ROOT/scripts/cargo-features.bash" "$repo/scripts/"
  cp "$REPO_ROOT/scripts/clippy-changed.sh" "$REPO_ROOT/scripts/doc-changed.sh" "$repo/scripts/"
  printf '%s\n' 'pub fn run() {}' > "$repo/crates/core/src/lib.rs"
  git -C "$repo" add -A
  git -C "$repo" commit -qm "Base state"
  printf '%s' "$repo"
}

run_against_real_git() {
  local repo="$1"
  local script="$2"
  local base="$3"

  : > "$CARGO_LOG"
  (
    cd "$repo" &&
      PATH="$CARGO_ONLY_BIN:$PATH" \
        CARGO_CI_PROFILE=nextest \
        CARGO_LOG="$CARGO_LOG" \
        CHANGED_BASE_SHA="$base" \
        bash "scripts/$script"
  ) > "$RUST_CHECK_LOG" 2>&1 || true
}

feature_repo=$(real_git_repo "feature-only")
feature_base=$(git -C "$feature_repo" rev-parse HEAD)
printf '%s\n' '# widen the shared set' >> "$feature_repo/scripts/cargo-features.bash"
git -C "$feature_repo" commit -aqm "Change the shared feature set"
for script in clippy-changed.sh doc-changed.sh; do
  run_against_real_git "$feature_repo" "$script" "$feature_base"
  grep -Fq -- "--workspace" "$CARGO_LOG" ||
    fail "Feature-only change did not reach a full workspace run through CHANGED_BASE_SHA: $script"
done

# A feature change alongside one crate must still widen to the workspace rather
# than scope to that crate.
mixed_repo=$(real_git_repo "feature-and-crate")
mixed_base=$(git -C "$mixed_repo" rev-parse HEAD)
printf '%s\n' '# widen the shared set' >> "$mixed_repo/scripts/cargo-features.bash"
printf '%s\n' 'pub fn added() {}' >> "$mixed_repo/crates/core/src/lib.rs"
git -C "$mixed_repo" commit -aqm "Change the feature set and one crate"
for script in clippy-changed.sh doc-changed.sh; do
  run_against_real_git "$mixed_repo" "$script" "$mixed_base"
  grep -Fq -- "--workspace" "$CARGO_LOG" ||
    fail "Feature change with a crate change did not widen to the workspace: $script"
done

# A command substitution inside a here-string does not trip errexit, so a missing
# or empty feature definition must be rejected explicitly rather than reaching
# cargo as an empty feature list.
missing_repo=$(real_git_repo "missing-feature-script")
missing_base=$(git -C "$missing_repo" rev-parse HEAD)
printf '%s\n' 'pub fn added() {}' >> "$missing_repo/crates/core/src/lib.rs"
git -C "$missing_repo" commit -aqm "Touch a crate"
rm "$missing_repo/scripts/cargo-features.bash"
for script in clippy-changed.sh doc-changed.sh; do
  run_against_real_git "$missing_repo" "$script" "$missing_base"
  [[ ! -s "$CARGO_LOG" ]] || fail "Missing feature definition still invoked cargo: $script"
done

empty_repo=$(real_git_repo "empty-feature-script")
empty_base=$(git -C "$empty_repo" rev-parse HEAD)
printf '%s\n' 'pub fn added() {}' >> "$empty_repo/crates/core/src/lib.rs"
git -C "$empty_repo" commit -aqm "Touch a crate"
printf '%s\n' '#!/usr/bin/env bash' 'exit 0' > "$empty_repo/scripts/cargo-features.bash"
for script in clippy-changed.sh doc-changed.sh; do
  run_against_real_git "$empty_repo" "$script" "$empty_base"
  [[ ! -s "$CARGO_LOG" ]] || fail "Empty feature definition still invoked cargo: $script"
done

grep -Fq '            crates/network/.*\.rs|' "$REPO_ROOT/.pre-commit-config.yaml" ||
  fail "Non-Linux network Clippy hook does not select Rust source"
grep -Fq '            crates/network/Cargo\.toml|' "$REPO_ROOT/.pre-commit-config.yaml" ||
  fail "Non-Linux network Clippy hook is not limited to Rust build inputs"

for config in \
  .config/nextest.toml \
  .cargo/audit.toml \
  .nautilus-engineering/tools.toml \
  deny.toml \
  tools.toml \
  crates/model/cbindgen.toml; do
  run_changed_script clippy-changed.sh "$config"
  [[ ! -s "$CARGO_LOG" ]] || fail "Non-build TOML triggered Clippy: $config"
  grep -Fq "No Rust build inputs detected; skipping clippy" "$RUST_CHECK_LOG" ||
    fail "Clippy did not report its non-build TOML skip: $config"

  run_changed_script doc-changed.sh "$config"
  [[ ! -s "$CARGO_LOG" ]] || fail "Non-build TOML triggered Cargo doc: $config"
  grep -Fq "No Rust build inputs detected; skipping cargo doc" "$RUST_CHECK_LOG" ||
    fail "Cargo doc did not report its non-build TOML skip: $config"
done

run_changed_script clippy-changed.sh "python/pyproject.toml"
grep -Fq "clippy --workspace" "$CARGO_LOG" ||
  fail "Python manifest did not trigger workspace Clippy"

run_changed_script doc-changed.sh "python/pyproject.toml"
grep -Fq "doc --workspace" "$CARGO_LOG" ||
  fail "Python manifest did not trigger workspace Cargo doc"

mixed_rust_inputs=$(printf '%s\n' "python/pyproject.toml" "crates/model/src/lib.rs")

run_changed_script clippy-changed.sh "$mixed_rust_inputs"
grep -Fq \
  "clippy -p nautilus-model --lib --bins --tests --profile nextest -- -D warnings" \
  "$CARGO_LOG" || fail "Python manifest escalated crate-scoped Clippy"

run_changed_script doc-changed.sh "$mixed_rust_inputs"
grep -Fq \
  "doc -p nautilus-model --no-deps --quiet --profile nextest" \
  "$CARGO_LOG" || fail "Python manifest escalated crate-scoped Cargo doc"

run_changed_script clippy-changed.sh "crates/model/src/lib.rs"
grep -Fq \
  "clippy -p nautilus-model --lib --bins --tests --profile nextest -- -D warnings" \
  "$CARGO_LOG" || fail "Crate Rust change did not select its Clippy package"

run_changed_script doc-changed.sh "crates/model/src/lib.rs"
grep -Fq \
  "doc -p nautilus-model --no-deps --quiet --profile nextest" \
  "$CARGO_LOG" || fail "Crate Rust change did not select its Cargo doc package"

run_changed_script clippy-changed.sh "Cargo.lock"
grep -Fq "clippy --workspace" "$CARGO_LOG" || fail "Cargo.lock did not trigger workspace Clippy"

run_changed_script doc-changed.sh "Cargo.lock"
grep -Fq "doc --workspace" "$CARGO_LOG" || fail "Cargo.lock did not trigger workspace Cargo doc"

run_changed_script clippy-changed.sh "clippy.toml"
grep -Fq "clippy --workspace" "$CARGO_LOG" || fail "Clippy config did not trigger workspace Clippy"

run_changed_script doc-changed.sh "clippy.toml"
[[ ! -s "$CARGO_LOG" ]] || fail "Clippy config triggered Cargo doc"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  '' \
  'set -euo pipefail' \
  '' \
  'printf "%s\n" "$*" >> "${GIT_LOG:?}"' \
  'if [ "${1:-}" = "rev-parse" ]; then' \
  '  echo develop' \
  '  exit 0' \
  'fi' \
  'if [ "${1:-}" != "diff" ]; then' \
  '  echo "Unexpected git command: $*" >&2' \
  '  exit 1' \
  'fi' \
  'if [ "${POSTGRES_INPUT_CHANGED:?}" = "true" ]; then' \
  '  exit 1' \
  'fi' \
  'exit 0' > "$MOCK_BIN/git"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  '' \
  'set -euo pipefail' \
  '' \
  'printf "%s\n" "$*" >> "${MAKE_LOG:?}"' > "$MOCK_BIN/make"
chmod +x "$MOCK_BIN/git" "$MOCK_BIN/make"

: > "$GIT_LOG"
: > "$MAKE_LOG"
PATH="$MOCK_BIN:$PATH" \
  GIT_LOG="$GIT_LOG" \
  MAKE_LOG="$MAKE_LOG" \
  POSTGRES_INPUT_CHANGED=false \
  "$MAKE_BIN" -C "$REPO_ROOT" --no-print-directory \
  MAKE="$MOCK_BIN/make" cargo-test-postgres-changed > /dev/null
[[ ! -s "$MAKE_LOG" ]] || fail "PostgreSQL bootstrap tests ran without related changes"
grep -Fq \
  "diff --cached --quiet -- schema/sql crates/infrastructure/src/sql/pg.rs crates/infrastructure/tests/integration/test_cache_database_postgres.rs crates/cli/src/database crates/cli/src/bin/cli.rs crates/cli/src/lib.rs crates/cli/src/opt.rs scripts/ci/test-postgres-bootstrap.bash" \
  "$GIT_LOG" || fail "PostgreSQL bootstrap change detection does not cover its inputs"

PATH="$MOCK_BIN:$PATH" \
  GIT_LOG="$GIT_LOG" \
  MAKE_LOG="$MAKE_LOG" \
  POSTGRES_INPUT_CHANGED=true \
  "$MAKE_BIN" -C "$REPO_ROOT" --no-print-directory \
  MAKE="$MOCK_BIN/make" cargo-test-postgres-changed > /dev/null
grep -Fxq -- "--no-print-directory cargo-test-postgres-ci" "$MAKE_LOG" ||
  fail "Related changes did not trigger PostgreSQL bootstrap tests"

echo "Build artifact reuse script tests passed"
