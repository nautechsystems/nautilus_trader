# Variables
# -----------------------------------------------------------------------------
PROJECT?=nautechsystems/nautilus_trader
REGISTRY?=ghcr.io/
IMAGE?=$(REGISTRY)$(PROJECT)
GIT_TAG:=$(shell git rev-parse --abbrev-ref HEAD)
IMAGE_FULL?=$(IMAGE):$(GIT_TAG)

# Shared and NautilusTrader-specific Cargo tool versions
CARGO_AUDIT_VERSION := $(shell bash scripts/cargo-tool-version.sh cargo-audit)
CARGO_CODSPEED_VERSION := $(shell bash scripts/cargo-tool-version.sh cargo-codspeed)
CARGO_DENY_VERSION := $(shell bash scripts/cargo-tool-version.sh cargo-deny)
CARGO_EDIT_VERSION := $(shell bash scripts/cargo-tool-version.sh cargo-edit)
CARGO_FUZZ_VERSION := $(shell bash scripts/cargo-tool-version.sh cargo-fuzz)
CARGO_HAWK_VERSION := $(shell bash scripts/cargo-tool-version.sh cargo-hawk)
CARGO_LLVM_COV_VERSION := $(shell bash scripts/cargo-tool-version.sh cargo-llvm-cov)
CARGO_MACHETE_VERSION := $(shell bash scripts/cargo-tool-version.sh cargo-machete)
CARGO_NEXTEST_VERSION := $(shell bash scripts/cargo-tool-version.sh cargo-nextest)
CARGO_VET_VERSION := $(shell bash scripts/cargo-tool-version.sh cargo-vet)
CBINDGEN_VERSION := $(shell bash scripts/cargo-tool-version.sh cbindgen)
FLAMEGRAPH_VERSION := $(shell bash scripts/cargo-tool-version.sh flamegraph)
LYCHEE_VERSION := $(shell bash scripts/cargo-tool-version.sh lychee)
# Shared and NautilusTrader-specific tool versions
PREK_VERSION := $(shell bash scripts/tool-version.sh prek)
NIGHTLY_TOOLCHAIN := $(shell bash scripts/tool-version.sh miri) # Pinned nightly, shared with Miri
DOCSRS_TOOLCHAIN := $(shell bash scripts/tool-version.sh nightly)
UV_VERSION := $(shell bash scripts/uv-version.sh)
UV_REQUIRED_SPEC := $(shell awk -F'"' '\
	/^\[tool\.uv\]/ { in_section=1; next } \
	/^\[/ { in_section=0 } \
	in_section && /^[[:space:]]*required-version[[:space:]]*=/ { print $$2; exit } \
' python/pyproject.toml)

V = 0  # 0 / 1 - verbose mode
Q = $(if $(filter 1,$V),,@) # Quiet mode, suppress command output
M = $(shell printf "\033[0;34m>\033[0m") # Message prefix for commands
empty :=
space := $(empty) $(empty)
comma := ,

# Nextest shows failures and the final summary unless verbose output is requested
NEXTEST_VERBOSE ?= false
ifeq ($(NEXTEST_VERBOSE),true)
NEXTEST_OUTPUT_ARGS := --verbose
else
NEXTEST_OUTPUT_ARGS := --status-level fail --final-status-level flaky
endif

# UV_SYNC_FLAGS controls whether uv keeps packages not managed by this project
# Set UV_SYNC_FLAGS= to make uv prune packages not in python/uv.lock
UV_SYNC_FLAGS ?= --inexact

# TARGET_DIR controls where Cargo places build artifacts
TARGET_DIR ?= $(CURDIR)/target

# Compiler configuration
# Uses clang by default (required by ed25519-blake2b and other deps).
# When sccache is available, wraps the compiler for build caching.
# Set CARGO_INCREMENTAL=0 with sccache for better cache hit rates.
# To disable sccache: make build SCCACHE=
SCCACHE ?= $(shell command -v sccache 2>/dev/null)

ifeq ($(SCCACHE),)
CC ?= clang
CXX ?= clang++
else
CC ?= sccache clang
CXX ?= sccache clang++
RUSTC_WRAPPER ?= sccache
CARGO_INCREMENTAL ?= 0
export RUSTC_WRAPPER
export CARGO_INCREMENTAL
endif

export CC
export CXX

# FAIL_FAST controls whether `cargo nextest` should stop after the first test
# failure. When set to `true` the `--no-fail-fast` flag is omitted so tests
# abort on the first failure. When `false` (the default) the flag is included
# allowing the full test suite to run.
FAIL_FAST ?= false

# NEXTEST_PROFILE selects the nextest profile from .config/nextest.toml.
# CI should set NEXTEST_PROFILE=ci to limit parallelism on resource-constrained runners.
NEXTEST_PROFILE ?= default

# Local Rust concurrency defaults are capped by host CPU count so lower-spec
# machines do not inherit settings meant for high-core workstations.
# Override with CARGO_BUILD_JOBS or NEXTEST_TEST_THREADS when needed
HOST_OS := $(shell uname -s)
HOST_CPU_COUNT := $(shell \
	n=`getconf _NPROCESSORS_ONLN 2>/dev/null` || n=; \
	if [ -z "$$n" ]; then n=`sysctl -n hw.ncpu 2>/dev/null` || n=; fi; \
	if [ -z "$$n" ]; then n="$${NUMBER_OF_PROCESSORS:-1}"; fi; \
	n=`printf '%s' "$$n" | tr -cd '0-9'`; \
	if [ -z "$$n" ]; then n=1; fi; \
	printf '%s' "$$n")

ifeq ($(CI),true)
LOCAL_CARGO_BUILD_JOBS_DEFAULT :=
LOCAL_NEXTEST_TEST_THREADS_DEFAULT :=
else
LOCAL_CARGO_BUILD_JOBS_DEFAULT := $(shell \
	n='$(HOST_CPU_COUNT)'; \
	[ "$$n" -gt 32 ] && n=32; \
	printf '%s' "$$n")
ifeq ($(NEXTEST_PROFILE),ci)
LOCAL_NEXTEST_TEST_THREADS_DEFAULT :=
else
LOCAL_NEXTEST_TEST_THREADS_DEFAULT := $(shell \
	n='$(HOST_CPU_COUNT)'; \
	[ "$$n" -gt 64 ] && n=64; \
	printf '%s' "$$n")
endif
endif

ifeq ($(origin CARGO_BUILD_JOBS),undefined)
CARGO_BUILD_JOBS_FOR_RUST := $(LOCAL_CARGO_BUILD_JOBS_DEFAULT)
else
CARGO_BUILD_JOBS_FOR_RUST := $(CARGO_BUILD_JOBS)
endif

ifeq ($(origin NEXTEST_TEST_THREADS),undefined)
NEXTEST_TEST_THREADS_FOR_RUST := $(LOCAL_NEXTEST_TEST_THREADS_DEFAULT)
else
NEXTEST_TEST_THREADS_FOR_RUST := $(NEXTEST_TEST_THREADS)
endif

# Doctests run under the libtest harness rather than nextest, so the same local
# concurrency cap is passed through as a harness argument.
ifneq ($(strip $(NEXTEST_TEST_THREADS_FOR_RUST)),)
DOCTEST_HARNESS_ARGS := -- --test-threads=$(NEXTEST_TEST_THREADS_FOR_RUST)
else
DOCTEST_HARNESS_ARGS :=
endif

# CARGO_CI_PROFILE selects the Cargo profile shared by Rust tests, stub generation,
# and debug Python builds.
CARGO_CI_PROFILE ?= nextest

PYTHON_EXTENSION_PATH := $(firstword $(wildcard \
	python/nautilus_trader/_libnautilus*.so \
	python/nautilus_trader/_libnautilus*.pyd))
PY_STUB_STAMP := $(TARGET_DIR)/.py-stubs.stamp
# Track input paths separately so additions and deletions also invalidate the stamp.
PY_STUB_INPUT_LIST := $(TARGET_DIR)/.py-stubs.inputs
PY_STUB_INPUT_LIST_COMMAND = { \
	printf '%s\n' .cargo/config.toml Cargo.lock Cargo.toml Makefile rust-toolchain.toml \
		python/generate_docstrings.py python/generate_stubs.py python/pyproject.toml python/uv.lock; \
	find crates -type f \( -name '*.rs' -o -name Cargo.toml \); \
	find patches/pyo3-stub-gen -type f; \
	find python/nautilus_trader -type f \( -name '*.py' -o -name '*.pyi' \); \
}
PY_STUB_INPUTS := $(shell $(PY_STUB_INPUT_LIST_COMMAND))

# Select the appropriate flag for `cargo nextest` depending on FAIL_FAST.
ifeq ($(FAIL_FAST),true)
FAIL_FAST_FLAG :=
else
FAIL_FAST_FLAG := --no-fail-fast
endif

# EXTRA_FEATURES allows adding optional features to cargo builds/tests.
# Can be set directly: make cargo-test EXTRA_FEATURES="capnp,hypersync"
# Or use convenience flags below for backwards compatibility.
EXTRA_FEATURES ?=

# HYPERSYNC is a convenience flag that adds hypersync to EXTRA_FEATURES.
# Can be overridden: make check-code HYPERSYNC=true
HYPERSYNC ?= false
ifeq ($(HYPERSYNC),true)
EXTRA_FEATURES += hypersync
endif

# DEFI controls whether defi feature is included (default: true).
# Can be disabled: make cargo-test-core DEFI=false
DEFI ?= true
ifeq ($(DEFI),true)
BASE_FEATURES := $(shell bash scripts/cargo-features.bash)
else
BASE_FEATURES := $(shell bash scripts/cargo-features.bash --no-defi)
endif

# $(shell) swallows a failing or missing script, and an empty list silently
# compiles every Rust gate with no features rather than failing.
ifeq ($(strip $(BASE_FEATURES)),)
$(error scripts/cargo-features.bash produced no features)
endif

# Combine base features with extra features
ifneq ($(strip $(EXTRA_FEATURES)),)
CARGO_FEATURES := $(BASE_FEATURES),$(EXTRA_FEATURES)
else
CARGO_FEATURES := $(BASE_FEATURES)
endif
CORE_SELECTED_FEATURE_LIST := $(filter-out hypersync,$(subst $(comma),$(space),$(CARGO_FEATURES)))
CORE_SELECTED_FEATURES := $(subst $(space),$(comma),$(strip $(CORE_SELECTED_FEATURE_LIST))),nautilus-serialization/sbe,nautilus-infrastructure/postgres

# Standard-precision (64-bit) selection, shared by the test and clippy targets.
# Two independent routes re-enable high precision, and both must be closed or the build
# silently runs high precision under a standard-precision name:
#   --no-default-features       most adapters declare default = [..., "high-precision"]
#   --exclude nautilus-blockchain   it depends on nautilus-model/defi, which implies high-precision
# `cargo tree` does not reflect either route reliably here. Verify a change by deleting an
# `#[allow(clippy::useless_conversion)]` in crates/model/src/types/quantity.rs and confirming
# clippy reports it under this selection.
STANDARD_PRECISION_ARGS := --workspace --exclude nautilus-blockchain --no-default-features --lib --tests --features "ffi,python"
SIM_PACKAGES := -p nautilus-common -p nautilus-core -p nautilus-event-store \
	-p nautilus-network -p nautilus-execution -p nautilus-live
SIM_FILTERSET := package(nautilus-common) + package(nautilus-event-store) + \
	package(nautilus-network) + \
	package(nautilus-execution) + \
	(package(nautilus-live) & test(test_startup_reconciliation_times_out_waiting_for_mass_status)) + \
	(package(nautilus-live) & test(~task::tests)) + \
	(package(nautilus-core) & test(~virtual_time))
SIM_HIGH_PRECISION_PACKAGES := -p nautilus-common -p nautilus-execution

# Pass the simulation cfg through `--config` rather than RUSTFLAGS, because the env var replaces
# the .cargo/config.toml rustflags while this joins with them, keeping -Dwarnings and the Linux
# link flags. It must sit on each subcommand's own command line, since cargo does not inherit a
# global `--config` into external subcommands such as clippy and nextest.
SIM_CARGO_CONFIG := --config 'target."cfg(all())".rustflags=["--cfg","madsim"]'

CARGO_BUILD_JOB_TARGETS := install install-debug build build-debug build-wheel py-stubs check-code \
	check-code-sim check-code-standard-precision \
	check-all-targets clippy clippy-fix clippy-fix-nightly clippy-pedantic-crate-% \
	clippy-strict-audit \
	docs docs-rust docsrs-check cargo-build cargo-check check-features hawk cargo-test \
	cargo-test-extras cargo-test-postgres-ci cargo-test-doc cargo-test-core-local cargo-test-core-selected \
	cargo-test-core cargo-test-adapters cargo-test-sim cargo-test-core-debug \
	cargo-test-core-local-debug cargo-test-lib cargo-test-standard-precision \
	cargo-test-debug cargo-test-coverage cargo-test-crate-% \
	cargo-test-coverage-crate-% cargo-test-coverage-html \
	cargo-test-coverage-crate-html-% cargo-miri-core cargo-miri-model \
	cargo-miri-plugin cargo-miri cargo-ci-benches cargo-codspeed-build \
	install-cli

# Apple ld can emit compact-unwind size warnings for large Rust binaries
# Rust source warnings remain denied by -Dwarnings in .cargo/config.toml
ifeq ($(HOST_OS),Darwin)
ifeq ($(origin CARGO_BUILD_WARNINGS),undefined)
$(CARGO_BUILD_JOB_TARGETS): export CARGO_BUILD_WARNINGS=warn
endif
endif

NEXTEST_ENV_TARGETS := cargo-test cargo-test-extras cargo-test-postgres-ci cargo-test-core-local \
	cargo-test-core-selected cargo-test-core cargo-test-adapters cargo-test-sim cargo-test-core-debug \
	cargo-test-core-local-debug cargo-test-lib cargo-test-standard-precision \
	cargo-test-debug cargo-test-coverage cargo-test-crate-% \
	cargo-test-coverage-crate-% cargo-test-coverage-html \
	cargo-test-coverage-crate-html-% cargo-miri-core cargo-miri-model \
	cargo-miri-plugin cargo-miri

ifneq ($(strip $(CARGO_BUILD_JOBS_FOR_RUST)),)
$(CARGO_BUILD_JOB_TARGETS): export CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS_FOR_RUST)
endif

ifneq ($(strip $(NEXTEST_TEST_THREADS_FOR_RUST)),)
$(NEXTEST_ENV_TARGETS): export NEXTEST_TEST_THREADS=$(NEXTEST_TEST_THREADS_FOR_RUST)
endif

# Core crates (excludes adapters/* and workspace members without tests)
CORE_CRATES := nautilus-analysis nautilus-backtest nautilus-common nautilus-core \
    nautilus-cryptography nautilus-data nautilus-event-store nautilus-execution \
    nautilus-indicators nautilus-infrastructure nautilus-live nautilus-model \
    nautilus-network nautilus-persistence nautilus-persistence-macros \
    nautilus-plugin nautilus-portfolio nautilus-risk nautilus-serialization \
    nautilus-system nautilus-testkit nautilus-trading

# Crates tested in the workspace-compiled adapter lane
ADAPTER_CRATES := nautilus-architect-ax nautilus-betfair nautilus-binance \
    nautilus-bitmex nautilus-blockchain nautilus-bybit nautilus-cli \
    nautilus-coinbase nautilus-databento nautilus-deribit nautilus-derive \
    nautilus-dydx nautilus-hyperliquid nautilus-interactive-brokers \
    nautilus-kraken nautilus-lighter nautilus-okx nautilus-polymarket \
    nautilus-sandbox nautilus-tardis

# Workspace members without Rust test functions:
# nautilus-trader is the container library, nautilus-pyo3 owns generated bindings,
# and nautilus-tutorials has a binary target with test = false.
NO_TEST_CRATES := nautilus-trader nautilus-pyo3 nautilus-tutorials

# > Colors
# Use ANSI escape codes directly for cross-platform compatibility (Git Bash on Windows doesn't have tput)
RED    := \033[0;31m
GREEN  := \033[0;32m
YELLOW := \033[0;33m
BLUE   := \033[0;34m
PURPLE := \033[0;35m
CYAN   := \033[0;36m
GRAY   := \033[0;37m
RESET  := \033[0m

.DEFAULT_GOAL := help

# Requires GNU Make across all platforms (Windows users should install it via MSYS2 or WSL).

#== Installation

.PHONY: sync
sync:  #-- Sync Python dependencies without building the package
	@if [ -z "$(UV_REQUIRED_SPEC)" ]; then \
		printf "$(RED)ERROR: Could not find required-version in python/pyproject.toml$(RESET)\n"; \
		exit 1; \
	fi
	@found="$$(uv --version 2>/dev/null | awk '{print $$2}' || true)"; \
	if [ -z "$$found" ]; then \
		printf "$(RED)ERROR: uv not found, $(UV_REQUIRED_SPEC) required; run \`make update-uv\` to install $(UV_VERSION).$(RESET)\n"; \
		exit 1; \
	fi
	$(info $(M) Syncing Python dependencies...)
	$Q cd python && VIRTUAL_ENV= uv sync --all-groups --all-extras --no-install-package nautilus-trader $(UV_SYNC_FLAGS)

.PHONY: install
install: build  #-- Install the package in release mode

.PHONY: install-debug
install-debug: build-debug  #-- Install the package in debug mode

#== Build

.PHONY: build
build: py-stubs  #-- Build and install the package in release mode
	$(info $(M) Building the Python extension in release mode...)
	$Q cd python && VIRTUAL_ENV= CARGO_TARGET_DIR=$(TARGET_DIR) uv run --no-sync maturin develop --release

.PHONY: build-debug
build-debug: py-stubs  #-- Build and install the package in debug mode
	$(info $(M) Building the Python extension in debug mode...)
	$Q cd python && VIRTUAL_ENV= CARGO_TARGET_DIR=$(TARGET_DIR) uv run --no-sync maturin develop --profile $(CARGO_CI_PROFILE)

.PHONY: build-wheel
build-wheel: sync  #-- Build a wheel distribution in release mode
	$(info $(M) Building the Python wheel in release mode...)
	$Q cd python && VIRTUAL_ENV= CARGO_TARGET_DIR=$(TARGET_DIR) uv run --no-sync maturin build --release --out ../dist

.PHONY: py-stub-input-list-force
py-stub-input-list-force:

$(PY_STUB_INPUT_LIST): py-stub-input-list-force
	$Q mkdir -p "$(dir $(PY_STUB_INPUT_LIST))"
	$Q py_stub_input_tmp="$(PY_STUB_INPUT_LIST).$$$$"; \
	$(PY_STUB_INPUT_LIST_COMMAND) | LC_ALL=C sort > "$$py_stub_input_tmp"; \
	if ! cmp -s "$$py_stub_input_tmp" "$(PY_STUB_INPUT_LIST)"; then \
		mv "$$py_stub_input_tmp" "$(PY_STUB_INPUT_LIST)"; \
	else \
		rm "$$py_stub_input_tmp"; \
	fi

$(PY_STUB_STAMP): $(PY_STUB_INPUTS) $(PY_STUB_INPUT_LIST) | sync
	$(info $(M) Generating Python type stubs...)
	$Q mkdir -p "$(dir $(PY_STUB_STAMP))"
	$Q cd python && VIRTUAL_ENV= NAUTILUS_STUB_PROFILE=$(CARGO_CI_PROFILE) \
		CARGO_TARGET_DIR=$(TARGET_DIR) uv run --no-sync python generate_stubs.py
	$Q touch "$(PY_STUB_STAMP)"

.PHONY: py-stubs
py-stubs: $(PY_STUB_STAMP)  #-- Regenerate Python type stubs when their inputs change

.PHONY: check-generated-drift
check-generated-drift:  #-- Check generated stubs and docstrings are committed
	$Q bash scripts/ci/check-generated-drift.bash

#== Clean

.PHONY: clean
clean: clean-build-artifacts clean-caches clean-builds  #-- Clean all build artifacts, caches, and builds

.PHONY: ib-stop
ib-stop:  #-- Stop local TWS/IBC processes and Docker IB Gateway containers
	@echo "Stopping local TWS/IBC processes..."
	@pkill -TERM -f "Trader Workstation" || true
	@pkill -TERM -f "ibcstart.sh" || true
	@pkill -TERM -f "displaybannerandlaunch.sh" || true
	@echo "Stopping Docker IB Gateway containers..."
	@docker ps --format '{{.Names}} {{.Image}}' | awk '/ib-gateway|ibgateway|Trader Workstation|tws/ {print $$1}' | xargs -r docker stop >/dev/null 2>&1 || true
	@sleep 2
	@pkill -KILL -f "Trader Workstation" || true
	@pkill -KILL -f "ibcstart.sh" || true
	@pkill -KILL -f "displaybannerandlaunch.sh" || true
	@docker ps --format '{{.Names}} {{.Image}}' | awk '/ib-gateway|ibgateway|Trader Workstation|tws/ {print $$1}' | xargs -r docker kill >/dev/null 2>&1 || true
	@echo "Done."

.PHONY: clean-builds
clean-builds:  #-- Clean distribution and target directories
	$Q rm -rf dist target \
		crates/adapters/derive/fuzz/target \
		crates/adapters/lighter/fuzz/target \
		crates/adapters/lighter/fuzz/pornin/target \
		2>/dev/null || true

.PHONY: clean-build-artifacts
clean-build-artifacts:  #-- Clean compiled artifacts (.so, .dll, and .pyc files)
	@echo "Cleaning build artifacts..."
	# Clean Rust build artifacts (keep final libraries)
	find target -name "*.rlib" -delete 2>/dev/null || true
	find target -name "*.rmeta" -delete 2>/dev/null || true
	rm -rf target/*/build target/*/deps 2>/dev/null || true
	# Clean Python build artifacts
	find . -type d -name "__pycache__" -not -path "*/.venv*" -exec rm -rf {} + 2>/dev/null || true
	find . -type f -a \( -name "*.pyc" -o -name "*.pyo" \) -not -path "*/.venv*" -exec rm -f {} + 2>/dev/null || true
	find . -type f -a \( -name "*.so" -o -name "*.dll" -o -name "*.dylib" \) -not -path "*/.venv*" -exec rm -f {} + 2>/dev/null || true
	rm -rf build/ 2>/dev/null || true
	# Clean test artifacts
	rm -rf .coverage .benchmarks 2>/dev/null || true

.PHONY: clean-caches
clean-caches:  #-- Clean pytest, ruff, uv, and cargo caches
	rm -rf .pytest_cache .ruff_cache python/.pytest_cache python/.ruff_cache 2>/dev/null || true
	-uv cache prune --force
	-cargo clean --workspace

.PHONY: distclean
distclean: clean  #-- Nuclear clean - remove all untracked files (requires FORCE=1)
	@if [ "$$FORCE" != "1" ]; then \
		echo "Pass FORCE=1 to really nuke"; \
		exit 1; \
	fi
	@echo "WARNING: removing all untracked files (git clean -fxd)..."
	git clean -fxd -e test_data/large/ -e test_data/local/ -e python/.venv/

#== Code Quality

.PHONY: format
format:  #-- Format Rust (with nightly) and Python code
	cargo +nightly fmt
	VIRTUAL_ENV= uv run --project python --no-sync ruff format . --config python/pyproject.toml --force-exclude

.PHONY: pre-commit
pre-commit:  #-- Run all pre-commit hooks on all files
	prek run --all-files

# The check-code target uses CARGO_FEATURES which is controlled by the HYPERSYNC flag.
# By default, hypersync is excluded to speed up checks. Override with: make check-code HYPERSYNC=true
.PHONY: check-code
check-code:  #-- Run clippy on lib/test targets and ruff --fix (use HYPERSYNC=true to include hypersync feature)
	$(info $(M) Running code quality checks...)
	@cargo clippy --workspace --lib --tests --features "$(CARGO_FEATURES)" --profile nextest -- -D warnings
	@VIRTUAL_ENV= uv run --project python --no-sync ruff check . --config python/pyproject.toml --fix --force-exclude
	@printf "$(GREEN)Checks passed$(RESET)\n"

.PHONY: check-code-standard-precision
check-code-standard-precision:  #-- Run clippy on lib/test targets with standard precision
	$(info $(M) Running standard-precision code quality checks...)
	@cargo clippy $(STANDARD_PRECISION_ARGS) --profile nextest -- -D warnings
	@printf "$(GREEN)Standard-precision checks passed$(RESET)\n"

.PHONY: check-code-sim
check-code-sim:  #-- Run clippy on DST simulation lib/test targets
	$(info $(M) Running DST simulation code quality checks...)
	@cargo clippy $(SIM_CARGO_CONFIG) $(SIM_PACKAGES) --lib --tests --features simulation --profile nextest -- -D warnings
	@printf "$(GREEN)DST simulation checks passed$(RESET)\n"

.PHONY: check-all-targets
check-all-targets:  #-- Run clippy on all targets including bins and examples (nightly)
	$(info $(M) Running full clippy on all targets...)
	@cargo clippy --workspace --all-targets --features "$(CARGO_FEATURES),examples" --profile nextest -- -D warnings
	@printf "$(GREEN)All-targets check passed$(RESET)\n"

# Time a block of make sub-targets. Use as:
#   @$(timer_start) \
#       $(MAKE) ... \
#       && $(MAKE) ... \
#   $(call timer_end,Time label)
# Prints "<Time label> time: H:MM:SS" and propagates the block's exit code.
timer_start = _t_start=$$(date +%s); (

define timer_end
); _t_rc=$$?; \
_t_elapsed=$$(( $$(date +%s) - _t_start )); \
printf "$(1) time: %d:%02d:%02d\n" $$(( _t_elapsed / 3600 )) $$(( (_t_elapsed % 3600) / 60 )) $$(( _t_elapsed % 60 )); \
exit $$_t_rc
endef

.PHONY: pre-flight
pre-flight: export CARGO_TARGET_DIR=$(TARGET_DIR)
pre-flight:  #-- Run pre-flight checks (format, tests, build, generated drift, and audit)
	$(info $(M) Running pre-flight checks...)
	@if ! git diff --quiet; then \
		printf "$(RED)ERROR: You have unstaged changes$(RESET)\n"; \
		printf "$(YELLOW)Stage your changes first:$(RESET) git add .\n"; \
		exit 1; \
	fi
	@$(timer_start) \
		$(MAKE) --no-print-directory sync \
		&& $(MAKE) --no-print-directory format \
		&& $(MAKE) --no-print-directory test-scripts-quiet \
		&& $(MAKE) --no-print-directory check-code EXTRA_FEATURES="capnp,hypersync" \
		&& $(MAKE) --no-print-directory check-code-sim \
		&& $(MAKE) --no-print-directory cargo-test-sim \
		&& $(MAKE) --no-print-directory cargo-test-extras \
		&& $(MAKE) --no-print-directory cargo-test-postgres-changed \
		&& $(MAKE) --no-print-directory build-debug \
		&& $(MAKE) --no-print-directory check-generated-drift \
		&& $(MAKE) --no-print-directory pytest \
		&& $(MAKE) --no-print-directory pytest-doctest ty \
		&& $(MAKE) --no-print-directory security-audit \
	$(call timer_end,Pre-flight)

.PHONY: ruff
ruff:  #-- Run ruff linter with automatic fixes
	VIRTUAL_ENV= uv run --project python --no-sync ruff check . --config python/pyproject.toml --fix --force-exclude

.PHONY: clippy
clippy:  #-- Run clippy linter (check only, workspace lints)
	cargo clippy --all-targets --all-features -- -D warnings

.PHONY: clippy-fix
clippy-fix:  #-- Run clippy linter with automatic fixes (workspace lints)
	cargo clippy --fix --all-targets --all-features --allow-dirty --allow-staged -- -D warnings

.PHONY: clippy-fix-nightly
clippy-fix-nightly:  #-- Run clippy linter with the pinned nightly toolchain and automatic fixes (workspace lints + additional strictness)
	# Work around rust-lang/rust#161495 in nightly-2026-08-23
	cargo +$(NIGHTLY_TOOLCHAIN) clippy \
		--config 'target."cfg(all())".rustflags=["-Znext-solver=coherence"]' \
		--fix --all-targets --all-features --allow-dirty --allow-staged -- -D warnings

.PHONY: clippy-strict-audit
clippy-strict-audit:  #-- Report candidate strict Clippy lints without failing on findings
	python3 -B scripts/clippy-strict-audit.py \
		--features "$(CARGO_FEATURES)" \
		--profile "$(CARGO_CI_PROFILE)"

.PHONY: clippy-pedantic-crate-%
clippy-pedantic-crate-%:  #-- Audit pedantic and panic-prone lints for one crate (usage: make clippy-pedantic-crate-<crate_name>)
	cargo clippy --all-targets --all-features -p $* -- -D warnings \
		-W clippy::pedantic \
		-W clippy::todo \
		-W clippy::unwrap_used \
		-W clippy::expect_used

.PHONY: hawk
hawk: check-hawk-installed  #-- Find unnecessary Rust public surface and restricted visibility
	$(info $(M) Running Hawk visibility checks...)
	cargo hawk check -D warnings

#== Dependencies

.PHONY: outdated
outdated: check-edit-installed  #-- Check for outdated dependencies
	sh scripts/check-outdated.sh
	@printf "\n$(CYAN)Checking tool versions...$(RESET)\n"
	@outdated_count=0; \
	for tool in cargo-audit:$(CARGO_AUDIT_VERSION) cargo-codspeed:$(CARGO_CODSPEED_VERSION) cargo-deny:$(CARGO_DENY_VERSION) cargo-edit:$(CARGO_EDIT_VERSION) cargo-fuzz:$(CARGO_FUZZ_VERSION) cargo-hawk:$(CARGO_HAWK_VERSION) cargo-llvm-cov:$(CARGO_LLVM_COV_VERSION) cargo-machete:$(CARGO_MACHETE_VERSION) cargo-nextest:$(CARGO_NEXTEST_VERSION) cargo-vet:$(CARGO_VET_VERSION) cbindgen:$(CBINDGEN_VERSION) flamegraph:$(FLAMEGRAPH_VERSION) lychee:$(LYCHEE_VERSION); do \
		name=$${tool%%:*}; current=$${tool##*:}; \
		latest=$$(cargo search $$name --limit 1 2>/dev/null | head -1 | awk -F\" '{print $$2}'); \
		if [ "$$current" != "$$latest" ]; then \
			printf "$(YELLOW)  $$name: $$current → $$latest$(RESET)\n"; \
			outdated_count=$$((outdated_count + 1)); \
		fi; \
	done; \
	[ $$outdated_count -eq 0 ] && printf "$(GREEN)  All tools up to date ✓$(RESET)\n"

.PHONY: update
update: cargo-update update-uv  #-- Update all dependencies (cargo and uv)
	$Q cd python && VIRTUAL_ENV= uv lock --upgrade

.PHONY: update-uv
update-uv:  #-- Install or upgrade uv to the version pinned in the shared tool catalog
	$(info $(M) Ensuring uv $(UV_VERSION) is installed...)
	@if [ "$$(uv --version 2>/dev/null | awk '{print $$2}')" = "$(UV_VERSION)" ]; then \
		printf "$(GREEN)uv $(UV_VERSION) already installed$(RESET)\n"; \
	else \
		curl -LsSf https://astral.sh/uv/$(UV_VERSION)/install.sh | sh; \
	fi

.PHONY: install-tools
install-tools: check-binstall-installed update-uv  #-- Install required development tools at shared and local pinned versions
	bash scripts/install-security-tools.sh \
	&& cargo install cargo-codspeed --version $(CARGO_CODSPEED_VERSION) --locked \
	&& cargo install cargo-edit --version $(CARGO_EDIT_VERSION) --locked \
	&& cargo install cargo-fuzz --version $(CARGO_FUZZ_VERSION) --locked \
	&& cargo binstall cargo-hawk --version $(CARGO_HAWK_VERSION) --no-confirm --locked \
	&& cargo install cargo-machete --version $(CARGO_MACHETE_VERSION) --locked \
	&& cargo install cargo-nextest --version $(CARGO_NEXTEST_VERSION) --locked \
	&& cargo install cargo-llvm-cov --version $(CARGO_LLVM_COV_VERSION) --locked \
	&& cargo install cbindgen --version $(CBINDGEN_VERSION) --locked \
	&& cargo install flamegraph --version $(FLAMEGRAPH_VERSION) --locked \
	&& cargo install lychee --version $(LYCHEE_VERSION) --locked \
	&& cargo binstall prek --version $(PREK_VERSION) --no-confirm --locked

#== Security

.PHONY: check-security-tools
check-security-tools:  #-- Verify supply-chain tools match the shared catalog
	VIRTUAL_ENV= uv run --project python --no-sync --no-build -- python scripts/security-audit.py check-tools

.PHONY: security-audit
security-audit:  #-- Run comprehensive security audit (cargo-audit, cargo-deny, cargo-vet, pip-audit, osv-scanner)
	$(info $(M) Running security audit...)
	VIRTUAL_ENV= uv run --project python --no-sync --no-build -- python scripts/security-audit.py run

.PHONY: cargo-deny
cargo-deny: check-deny-installed  #-- Run cargo-deny checks (advisories, sources, bans, licenses)
	cargo deny --all-features check

.PHONY: cargo-vet
cargo-vet: check-vet-installed  #-- Run cargo-vet supply chain audit
	cargo vet

#== Documentation

.PHONY: docs
docs: docs-python docs-rust  #-- Build all documentation (Python and Rust)

.PHONY: docs-python
docs-python:  #-- Build Python documentation with Sphinx
	VIRTUAL_ENV= uv run --project python --no-sync sphinx-build -M html ./docs/api_reference ./api_reference

# Path to extra HTML injected into every rustdoc <head>. Left empty here so the
# markup stays with whichever build supplies it, rather than living in this repo.
RUSTDOC_EXTRA_HEAD ?=

.PHONY: docs-rust
docs-rust: export RUSTDOCFLAGS=--enable-index-page -Zunstable-options $(if $(RUSTDOC_EXTRA_HEAD),--html-in-header $(RUSTDOC_EXTRA_HEAD))
docs-rust:  #-- Build Rust documentation with cargo doc
	cargo +nightly doc --all-features --no-deps --workspace

.PHONY: docsrs-check
docsrs-check: export DOCS_RS=1
docsrs-check: export RUSTDOCFLAGS=--cfg docsrs -D warnings
docsrs-check: check-hack-installed #-- Check documentation builds for docs.rs compatibility
	cargo +$(DOCSRS_TOOLCHAIN) hack --workspace --ignore-private --ignore-unknown-features \
		--features arrow,capnp,cloud,defi,display \
		--features example-databento,examples,ffi,high-precision,host \
		--features hypersync,indicators,live,node,persistence,plugin \
		--features postgres,redis,replay,sbe,simulation,streaming,test-support \
		--features tracing-bridge,transport-sockudo,turmoil \
		doc --no-deps

# markdownlint-cli2 version comes from the pre-commit hook rev so both agree.
MARKDOWNLINT_VERSION := $(shell awk '\
	/markdownlint-cli2/ { found=1 } \
	found && /^[[:space:]]*rev:[[:space:]]*/ { sub(/^v/, "", $$2); print $$2; exit } \
' .pre-commit-config.yaml)
MARKDOWNLINT ?= npx --yes markdownlint-cli2@$(MARKDOWNLINT_VERSION)
# File sets mirror the pre-commit scopes: the global patches exclusion for both,
# plus the markdownlint hook's own exclusions for the linter.
MARKDOWN_FILES = $(shell git ls-files '*.md' | grep -v '^patches/pyo3-stub-gen/')
MARKDOWNLINT_FILES = $(shell git ls-files '*.md' | \
	grep -vE '^(patches/pyo3-stub-gen/|CLA\.md$$|RELEASES\.md$$)')

.PHONY: check-markdown
check-markdown:  #-- Lint Markdown with markdownlint-cli2 and check table delimiter padding
	$(info $(M) Checking Markdown...)
	@$(MARKDOWNLINT) --config .markdownlint.jsonc $(MARKDOWNLINT_FILES)
	@python3 -B scripts/check-markdown-tables.py $(MARKDOWN_FILES)
	@printf "$(GREEN)Markdown check passed$(RESET)\n"

# Rust doc links are collected into Markdown so lychee parses their `[label](url)` form.
# A dot-directory keeps the file out of the `**/*.md` glob, which would otherwise read it twice.
DOC_LINKS = .tmp-doc-links/doc-links.md

LYCHEE_FLAGS = \
	--verbose \
	--no-progress \
	--exclude-all-private \
	--max-retries 3 \
	--retry-wait-time 5 \
	--timeout 30 \
	--max-concurrency 10 \
	--accept "100..=103,200..=299,429,502..=504"

.PHONY: docs-check-links
docs-check-links:  #-- Check for broken links in documentation (periodic audit)
	$(info $(M) Checking documentation links...)
	@git ls-files -- '*.rs' ':(exclude)patches/**' \
		| python3 -B scripts/extract-doc-links.py $(DOC_LINKS)
	@status=0; \
	lychee $(LYCHEE_FLAGS) \
		--include-fragments \
		--fallback-extensions md,py,html \
		--exclude-path .venv \
		--exclude-path target \
		--exclude-path docs/python-api-latest \
		--exclude "file://.*/python-api-latest/.*" \
		"**/*.md" "docs/**/*.py" || status=1; \
	lychee $(LYCHEE_FLAGS) $(DOC_LINKS) || status=1; \
	exit $$status
	@printf "$(GREEN)Link check passed$(RESET)\n"

#== Rust Development

.PHONY: cargo-build
cargo-build:  #-- Build Rust crates in release mode
	cargo build --release --all-features

.PHONY: cargo-update
cargo-update:  #-- Update Rust dependencies (versions from Cargo.toml)
	bash scripts/update-cargo-dependencies.bash

.PHONY: cargo-check
cargo-check:  #-- Check Rust code without building
	cargo check --workspace --all-features

# Security tool checks
.PHONY: check-deny-installed
check-deny-installed:  #-- Verify the pinned cargo-deny version is installed
	@if ! cargo deny --version >/dev/null 2>&1; then \
		printf "$(YELLOW)cargo-deny %s is required but not installed$(RESET)\n" \
			"$(CARGO_DENY_VERSION)"; \
		printf "Install with: $(CYAN)cargo install cargo-deny --version %s --locked$(RESET)\n" \
			"$(CARGO_DENY_VERSION)"; \
		exit 1; \
	fi
	@INSTALLED=$$(cargo deny --version | awk '{print $$2}'); \
	if [ "$$INSTALLED" != "$(CARGO_DENY_VERSION)" ]; then \
		printf "$(RED)cargo-deny version mismatch: installed %s, expected %s (from the shared tool catalog)$(RESET)\n" \
			"$$INSTALLED" "$(CARGO_DENY_VERSION)"; \
		printf "Install with: $(CYAN)cargo install cargo-deny --version %s --locked$(RESET)\n" \
			"$(CARGO_DENY_VERSION)"; \
		exit 1; \
	fi

.PHONY: check-binstall-installed
check-binstall-installed:  #-- Verify cargo-binstall is installed (one-off prerequisite for install-tools)
	@if ! command -v cargo-binstall >/dev/null 2>&1; then \
		printf "$(YELLOW)cargo-binstall is required but not installed$(RESET)\n"; \
		printf "Install once per machine with: $(CYAN)cargo install cargo-binstall --locked$(RESET)\n"; \
		printf "See: https://github.com/cargo-bins/cargo-binstall\n"; \
		exit 1; \
	fi

.PHONY: check-vet-installed
check-vet-installed:  #-- Verify cargo-vet is installed
	@if ! cargo vet --version >/dev/null 2>&1; then \
		echo "cargo-vet is not installed. You can install it using 'cargo install cargo-vet'"; \
		exit 1; \
	fi

# Testing tool checks
.PHONY: check-nextest-installed
check-nextest-installed:  #-- Verify cargo-nextest is installed
	@if ! cargo nextest --version >/dev/null 2>&1; then \
		echo "cargo-nextest is not installed. You can install it using 'cargo install cargo-nextest'"; \
		exit 1; \
	fi

.PHONY: check-llvm-cov-installed
check-llvm-cov-installed:  #-- Verify cargo-llvm-cov is installed
	@if ! cargo llvm-cov --version >/dev/null 2>&1; then \
		echo "cargo-llvm-cov is not installed. You can install it using 'cargo install cargo-llvm-cov'"; \
		exit 1; \
	fi

# Cargo utility checks
.PHONY: check-hack-installed
check-hack-installed:  #-- Verify cargo-hack is installed
	@if ! cargo hack --version >/dev/null 2>&1; then \
		echo "cargo-hack is not installed. You can install it using 'cargo install cargo-hack'"; \
		exit 1; \
	fi

.PHONY: check-edit-installed
check-edit-installed:  #-- Verify cargo-edit is installed
	@if ! cargo upgrade --version >/dev/null 2>&1; then \
		echo "cargo-edit is not installed. You can install it using 'cargo install cargo-edit'"; \
		exit 1; \
	fi

.PHONY: check-hawk-installed
check-hawk-installed:  #-- Verify the pinned cargo-hawk version is installed
	@if ! cargo hawk --version >/dev/null 2>&1 || ! command -v cargo-hawk-driver >/dev/null 2>&1; then \
		printf "$(YELLOW)cargo-hawk %s is required but not installed$(RESET)\n" \
			"$(CARGO_HAWK_VERSION)"; \
		printf "Install with: $(CYAN)cargo binstall cargo-hawk --version %s --no-confirm --locked$(RESET)\n" \
			"$(CARGO_HAWK_VERSION)"; \
		exit 1; \
	fi
	@INSTALLED=$$(cargo hawk --version | awk '{print $$3}'); \
	if [ "$$INSTALLED" != "$(CARGO_HAWK_VERSION)" ]; then \
		printf "$(RED)cargo-hawk version mismatch: installed %s, expected %s (from Cargo.toml)$(RESET)\n" \
			"$$INSTALLED" "$(CARGO_HAWK_VERSION)"; \
		exit 1; \
	fi

.PHONY: check-features
check-features: check-hack-installed  #-- Verify crate feature combinations compile correctly
	cargo hack --workspace check --each-feature --all-targets

.PHONY: check-cbindgen-abi
check-cbindgen-abi:  #-- Verify generated C headers preserve the public ABI names
	$(info $(M) Checking cbindgen C ABI...)
	$Q bash scripts/ci/check-cbindgen-abi.bash

.PHONY: check-capnp-schemas  #-- Verify Cap'n Proto schemas are up-to-date
check-capnp-schemas:
	$(info $(M) Checking if Cap'n Proto schemas are up-to-date...)
	@if ! command -v capnp > /dev/null 2>&1; then \
		echo "$(YELLOW)⚠ capnp not installed, skipping schema check$(RESET)"; \
	elif ! CAPNP_CHECK=1 bash scripts/regen-capnp.sh; then \
		echo "$(RED)Error: Cap'n Proto regeneration failed$(RESET)"; \
		echo "Run manually to see errors: ./scripts/regen-capnp.sh"; \
		exit 1; \
	else \
		DIFF_OUTPUT="$$(git diff -I\"ENCODED_NODE\" -- crates/serialization/generated/capnp)"; \
		if [ -n "$$DIFF_OUTPUT" ]; then \
			echo "$(RED)Error: Cap'n Proto generated files are out of date$(RESET)"; \
			echo "Please run: ./scripts/regen-capnp.sh"; \
			echo "Or: make regen-capnp"; \
			exit 1; \
		else \
			echo "$(GREEN)✓ Cap'n Proto schemas are up-to-date$(RESET)"; \
		fi; \
	fi

.PHONY: regen-capnp  #-- Regenerate Cap'n Proto schema files
regen-capnp:
	$(info $(M) Regenerating Cap'n Proto schemas...)
	@bash scripts/regen-capnp.sh

.PHONY: check-docker-toolchain-pins
check-docker-toolchain-pins:  #-- Check Docker toolchain pins
	$(info $(M) Checking Docker toolchain pins...)
	$Q bash scripts/ci/check-docker-toolchain-pins.bash

.PHONY: check-github-action-pins
check-github-action-pins:  #-- Check GitHub Action pins
	$(info $(M) Checking GitHub Action pins...)
	$Q bash scripts/ci/check-github-action-shas.sh \
		$$(git ls-files \
			'.github/actions/**/action.yml' \
			'.github/actions/**/action.yaml' \
			'.github/workflows/*.yml' \
			'.github/workflows/*.yaml')

.PHONY: check-jiff-features
check-jiff-features:  #-- Check jiff features
	$(info $(M) Checking jiff features...)
	$Q bash .pre-commit-hooks/check_jiff_features.sh

.PHONY: test-scripts
test-scripts:  #-- Run repository script tests
	$(info $(M) Running script tests...)
	$Q bash .pre-commit-hooks/test_cargo_machete.sh
	$Q bash .pre-commit-hooks/test_check_cargo_conventions.sh
	$Q bash .pre-commit-hooks/test_check_docs_conventions.sh
	$Q bash .pre-commit-hooks/test_check_dst_conventions.sh
	$Q bash .pre-commit-hooks/test_check_formatting_py.sh
	$Q bash .pre-commit-hooks/test_check_formatting_rs.sh
	$Q bash .pre-commit-hooks/test_check_jiff_features.sh
	$Q bash .pre-commit-hooks/test_check_logging_conventions.sh
	$Q bash .pre-commit-hooks/test_check_pyo3_conventions.sh
	$Q bash .pre-commit-hooks/test_check_unicode_typography.sh
	$Q bash .pre-commit-hooks/test_check_ustr_conventions.sh
	$Q bash scripts/ci/test-build-artifact-reuse.bash
	$Q bash scripts/ci/test-check-docker-toolchain-pins.bash
	$Q bash scripts/ci/test-check-miri-toolchain.bash
	$Q bash scripts/ci/test-check-nightly-merge-status.bash
	$Q bash scripts/ci/test-check-workspace-test-coverage.bash
	$Q bash scripts/ci/test-configure-r2-aws.bash
	$Q bash scripts/ci/test-docker-workflow-scripts.bash
	$Q bash scripts/ci/test-nightly-merge-workflow.bash
	$Q bash scripts/ci/test-package-cli-artifact.bash
	$Q bash scripts/ci/test-plan.bash
	$Q bash scripts/ci/test-publish-cargo-crates-check.bash
	$Q bash scripts/ci/test-publish-cli-r2-upload-installer.bash
	$Q bash scripts/ci/test-publish-wheels.bash
	$Q bash scripts/ci/test-release-github-assets.bash
	$Q bash scripts/ci/test-release-verification-retry.bash
	$Q bash scripts/ci/test-select-attestation-bundle.bash
	$Q bash scripts/ci/test-tool-version-scripts.bash
	$Q bash scripts/ci/test-validate-wheel-upload.bash
	$Q bash scripts/ci/test-verify-published-registries-crates.bash
	$Q bash scripts/test-check-cargo-cooldown.bash
	$Q bash scripts/test-clippy-strict-audit.bash
	$Q bash scripts/test-update-cargo-dependencies.bash
	$Q python3 -B scripts/ci/test_check_commit_message.py
	@printf "$(GREEN)Script tests passed$(RESET)\n"

.PHONY: test-scripts-quiet
test-scripts-quiet:
	@test_scripts_log="$$(mktemp "$${TMPDIR:-/tmp}/nautilus-script-tests.XXXXXX")"; \
	trap 'rm -f "$$test_scripts_log"' EXIT; \
	if $(MAKE) --no-print-directory test-scripts > "$$test_scripts_log" 2>&1; then \
		:; \
	else \
		status=$$?; \
		cat "$$test_scripts_log" >&2; \
		exit $$status; \
	fi

#== Rust Testing

.PHONY: cargo-test
cargo-test: export RUST_BACKTRACE=1
cargo-test: check-nextest-installed
cargo-test:  #-- Run all Rust tests (use EXTRA_FEATURES="feature1 feature2" or HYPERSYNC=true)
ifeq ($(NEXTEST_VERBOSE),true)
	$(info $(M) Running Rust tests with verbose output...)
	cargo nextest run --workspace --lib --tests --features "$(CARGO_FEATURES)" $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile $(CARGO_CI_PROFILE) $(NEXTEST_OUTPUT_ARGS)
else
	$(info $(M) Running Rust tests (showing summary and failures only)...)
	cargo nextest run --workspace --lib --tests --features "$(CARGO_FEATURES)" $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile $(CARGO_CI_PROFILE) $(NEXTEST_OUTPUT_ARGS)
endif

.PHONY: cargo-test-extras
cargo-test-extras:  #-- Run all Rust tests with capnp and hypersync features (convenience shortcut)
	$(MAKE) cargo-test EXTRA_FEATURES="capnp,hypersync"

.PHONY: cargo-test-postgres-ci
cargo-test-postgres-ci: export RUST_BACKTRACE=1
cargo-test-postgres-ci: check-nextest-installed
cargo-test-postgres-ci:  #-- Run focused PostgreSQL tests with the CI bootstrap role split
	$(info $(M) Running PostgreSQL bootstrap tests...)
	NEXTEST_PROFILE="$(NEXTEST_PROFILE)" \
	NEXTEST_VERBOSE="$(NEXTEST_VERBOSE)" \
	CARGO_CI_PROFILE="$(CARGO_CI_PROFILE)" \
	POSTGRES_TEST_FEATURES="$(BASE_FEATURES),capnp,hypersync" \
	bash scripts/ci/test-postgres-bootstrap.bash

POSTGRES_BOOTSTRAP_INPUTS := schema/sql \
	crates/infrastructure/src/sql/pg.rs \
	crates/infrastructure/tests/integration/test_cache_database_postgres.rs \
	crates/cli/src/database \
	crates/cli/src/bin/cli.rs \
	crates/cli/src/lib.rs \
	crates/cli/src/opt.rs \
	scripts/ci/test-postgres-bootstrap.bash

.PHONY: cargo-test-postgres-changed
cargo-test-postgres-changed:  #-- Run PostgreSQL bootstrap tests when related staged files change
	@if git diff --cached --quiet -- $(POSTGRES_BOOTSTRAP_INPUTS); then \
		printf "$(YELLOW)Skipping PostgreSQL bootstrap tests: no related staged files changed$(RESET)\n"; \
	else \
		$(MAKE) --no-print-directory cargo-test-postgres-ci; \
	fi

# Doctests need their own target because `cargo nextest` cannot run them.
# The scheduled nightly test workflow runs them separately from regular CI.
.PHONY: cargo-test-doc
cargo-test-doc: export RUST_BACKTRACE=1
cargo-test-doc:  #-- Run Rust doctests (examples in `///` and `//!` comments)
	$(info $(M) Running Rust doctests...)
	@doctest_log="$$(mktemp "$${TMPDIR:-/tmp}/nautilus-doctest.XXXXXX")"; \
	trap 'rm -f "$$doctest_log"' EXIT; \
	if cargo test --quiet --doc --workspace --features "$(CARGO_FEATURES)" --profile $(CARGO_CI_PROFILE) $(FAIL_FAST_FLAG) $(DOCTEST_HARNESS_ARGS) >"$$doctest_log" 2>&1; then \
		:; \
	else \
		status=$$?; \
		cat "$$doctest_log"; \
		exit $$status; \
	fi

# Both core and adapter targets use identical --workspace --features flags so
# cargo sees the same feature union and does not recompile between runs.
# The -E filterset selects which tests to execute.
CORE_FILTERSET := $(subst $(eval ) , + ,$(foreach crate,$(CORE_CRATES),package($(crate))))
ADAPTER_FILTERSET := $(subst $(eval ) , + ,$(foreach crate,$(ADAPTER_CRATES),package($(crate))))

.PHONY: cargo-test-core-local
cargo-test-core-local: export RUST_BACKTRACE=1
cargo-test-core-local: check-nextest-installed
cargo-test-core-local:  #-- Run Rust tests for core crates only with direct package selection (fast local compile)
ifeq ($(NEXTEST_VERBOSE),true)
	$(info $(M) Running Rust tests for core crates with direct package selection...)
	cargo nextest run $(foreach crate,$(CORE_CRATES),-p $(crate)) --lib --tests --features "$(CORE_SELECTED_FEATURES)" $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile $(CARGO_CI_PROFILE) $(NEXTEST_OUTPUT_ARGS)
else
	$(info $(M) Running Rust tests for core crates with direct package selection (showing summary and failures only)...)
	cargo nextest run $(foreach crate,$(CORE_CRATES),-p $(crate)) --lib --tests --features "$(CORE_SELECTED_FEATURES)" $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile $(CARGO_CI_PROFILE) $(NEXTEST_OUTPUT_ARGS)
endif

.PHONY: cargo-test-core-selected
# CI uses direct package selection so core jobs do not compile adapter test binaries.
# This intentionally avoids workspace feature unification from adapter crates.
cargo-test-core-selected: cargo-test-core-local  #-- Run Rust tests for core crates with direct package selection

.PHONY: cargo-test-core
cargo-test-core: export RUST_BACKTRACE=1
cargo-test-core: check-nextest-installed
cargo-test-core:  #-- Run Rust tests for core crates with workspace compilation
ifeq ($(NEXTEST_VERBOSE),true)
	$(info $(M) Running Rust tests for core crates...)
	cargo nextest run --workspace --lib --tests --features "$(CARGO_FEATURES)" -E '$(CORE_FILTERSET)' $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile $(CARGO_CI_PROFILE) $(NEXTEST_OUTPUT_ARGS)
else
	$(info $(M) Running Rust tests for core crates (showing summary and failures only)...)
	cargo nextest run --workspace --lib --tests --features "$(CARGO_FEATURES)" -E '$(CORE_FILTERSET)' $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile $(CARGO_CI_PROFILE) $(NEXTEST_OUTPUT_ARGS)
endif

.PHONY: cargo-test-adapters
cargo-test-adapters: export RUST_BACKTRACE=1
cargo-test-adapters: check-nextest-installed
cargo-test-adapters:  #-- Run Rust tests for the workspace-compiled adapter lane
ifeq ($(NEXTEST_VERBOSE),true)
	$(info $(M) Running Rust tests for the workspace-compiled adapter lane...)
	cargo nextest run --workspace --lib --tests --features "$(CARGO_FEATURES)" -E '$(ADAPTER_FILTERSET)' $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile $(CARGO_CI_PROFILE) $(NEXTEST_OUTPUT_ARGS)
else
	$(info $(M) Running Rust tests for the workspace-compiled adapter lane (showing summary and failures only)...)
	cargo nextest run --workspace --lib --tests --features "$(CARGO_FEATURES)" -E '$(ADAPTER_FILTERSET)' $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile $(CARGO_CI_PROFILE) $(NEXTEST_OUTPUT_ARGS)
endif

# DST simulation smoke test. Nextest compiles every selected lib/test target
# before applying its filter, so the standard-precision run is also the compile
# gate without a separate build. Two feature-coherent runs execute every test
# that is sim-compatible today: all of nautilus-common, nautilus-event-store,
# nautilus-network, and nautilus-execution. Transport-bound and thread-blocking
# tests are gated out at the source. The lane also runs the LiveNode startup
# reconciliation timeout regression and the cross-crate seam pinning tests in
# nautilus-core.
# Each leg runs with the standard fixed-precision build first, then again
# under `high-precision` for the crates that consume `nautilus-model` types,
# so the seam-routed code paths are exercised under both `QuantityRaw` /
# `PriceRaw` widths (u64 vs u128). See docs/concepts/dst.md for the full
# DST scope.
.PHONY: cargo-test-sim
cargo-test-sim: export RUST_BACKTRACE=1
cargo-test-sim: check-nextest-installed
cargo-test-sim:  #-- Run DST simulation smoke tests (cfg madsim + simulation feature)
	$(info $(M) Running in-scope DST tests under simulation...)
	cargo nextest run $(SIM_CARGO_CONFIG) $(SIM_PACKAGES) --lib --tests --features simulation -E '$(SIM_FILTERSET)' $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile $(CARGO_CI_PROFILE) $(NEXTEST_OUTPUT_ARGS)
	$(info $(M) Running precision-sensitive DST tests under simulation + high-precision...)
	cargo nextest run $(SIM_CARGO_CONFIG) $(SIM_HIGH_PRECISION_PACKAGES) --lib --tests --features "simulation,high-precision" $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile $(CARGO_CI_PROFILE) $(NEXTEST_OUTPUT_ARGS)

.PHONY: cargo-test-core-debug
cargo-test-core-debug: export RUST_BACKTRACE=1
cargo-test-core-debug: check-nextest-installed
cargo-test-core-debug:  #-- Run Rust tests for core crates (debug profile)
	cargo nextest run --workspace --lib --tests --features "$(CARGO_FEATURES)" -E '$(CORE_FILTERSET)' $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) $(NEXTEST_OUTPUT_ARGS)

.PHONY: cargo-test-core-local-debug
cargo-test-core-local-debug: export RUST_BACKTRACE=1
cargo-test-core-local-debug: check-nextest-installed
cargo-test-core-local-debug:  #-- Run Rust tests for core crates with direct package selection (debug profile)
	cargo nextest run $(foreach crate,$(CORE_CRATES),-p $(crate)) --lib --tests --features "$(CORE_SELECTED_FEATURES)" $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) $(NEXTEST_OUTPUT_ARGS)

.PHONY: cargo-test-lib
cargo-test-lib: export RUST_BACKTRACE=1
cargo-test-lib: check-nextest-installed
cargo-test-lib:  #-- Run Rust library tests only with high precision
	cargo nextest run --lib --workspace --no-default-features --features "$(BASE_FEATURES),test-support" $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile $(CARGO_CI_PROFILE) $(NEXTEST_OUTPUT_ARGS)

.PHONY: cargo-test-standard-precision
cargo-test-standard-precision: export RUST_BACKTRACE=1
cargo-test-standard-precision: check-nextest-installed
cargo-test-standard-precision:  #-- Run Rust tests with standard precision (debug profile)
	cargo nextest run $(STANDARD_PRECISION_ARGS) $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) $(NEXTEST_OUTPUT_ARGS)

.PHONY: cargo-test-debug
cargo-test-debug: export RUST_BACKTRACE=1
cargo-test-debug: check-nextest-installed
cargo-test-debug:  #-- Run Rust tests with high precision (debug profile)
	cargo nextest run --workspace --lib --tests --features "$(BASE_FEATURES)" $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) $(NEXTEST_OUTPUT_ARGS)

.PHONY: cargo-test-coverage
cargo-test-coverage: check-nextest-installed check-llvm-cov-installed
cargo-test-coverage:  #-- Run Rust tests with coverage reporting
	cargo llvm-cov nextest run --workspace --lib --tests --features "$(CARGO_FEATURES)"

# -----------------------------------------------------------------------------
# Library tests for a single crate
# -----------------------------------------------------------------------------
# Invoke as:
#   make cargo-test-crate-<crate_name>
# Examples:
#   make cargo-test-crate-nautilus-model
#   make cargo-test-crate-nautilus-live
#
# Enables all crate features except extension-module (which requires a Python
# interpreter at link time). Feature list is resolved by crate-test-features.sh.
# -----------------------------------------------------------------------------

.PHONY: cargo-test-crate-%
cargo-test-crate-%: export RUST_BACKTRACE=1
cargo-test-crate-%: check-nextest-installed
cargo-test-crate-%:  #-- Run Rust tests for a specific crate (usage: make cargo-test-crate-<crate_name>)
	cargo nextest run --lib $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile $(CARGO_CI_PROFILE) -p $* --features "$$(./scripts/crate-test-features.sh $*)" $(NEXTEST_OUTPUT_ARGS)

.PHONY: cargo-test-coverage-crate-%
cargo-test-coverage-crate-%: export RUST_BACKTRACE=1
cargo-test-coverage-crate-%: check-nextest-installed check-llvm-cov-installed
cargo-test-coverage-crate-%:  #-- Run Rust tests with coverage reporting for a specific crate (usage: make cargo-test-coverage-crate-<crate_name>)
	cargo llvm-cov nextest --lib $(FAIL_FAST_FLAG) --cargo-profile nextest -p $* $(if $(FEATURES),--features "$(FEATURES)")

.PHONY: cargo-test-coverage-html
cargo-test-coverage-html: check-nextest-installed check-llvm-cov-installed
cargo-test-coverage-html:  #-- Run Rust tests with HTML coverage report (opens in browser)
	cargo llvm-cov nextest --workspace --lib --tests --features "$(CARGO_FEATURES)" --html --open

.PHONY: cargo-test-coverage-crate-html-%
cargo-test-coverage-crate-html-%: export RUST_BACKTRACE=1
cargo-test-coverage-crate-html-%: check-nextest-installed check-llvm-cov-installed
cargo-test-coverage-crate-html-%:  #-- Run coverage for specific crate with HTML report (usage: make cargo-test-coverage-crate-html-<crate_name>)
	cargo llvm-cov nextest --lib $(FAIL_FAST_FLAG) --cargo-profile nextest -p $* $(if $(FEATURES),--features "$(FEATURES)") --html --open

# -----------------------------------------------------------------------------
# Miri (UB detection)
# -----------------------------------------------------------------------------
# Runs library and selected integration tests under Miri to detect undefined
# behaviour: invalid pointer operations, aliasing violations (Stacked/Tree
# Borrows), uninitialised reads, and unsound `unsafe` impls. Requires a nightly
# toolchain with the `miri` component installed.
#
# Features: `ffi`, `python`, `extension-module`, and `defi` are intentionally
# disabled. Miri cannot execute Python interpreter calls or most foreign FFI,
# and `defi` pulls in `alloy-primitives`, which is out of scope here. The
# `--lib` filter keeps doctests out of the run as well.
#
# Proptest cases are dialled down via `PROPTEST_CASES` since Miri is roughly
# 10-100x slower than native execution. `MIRIFLAGS` enables disable-isolation
# so tests that read environment variables (e.g. PATH probes) work. Most runs
# use strict provenance; the collections slice uses permissive provenance to
# match arc-swap's Miri policy.
# -----------------------------------------------------------------------------

# Override these on the command line if needed, e.g.:
#   make cargo-miri-core MIRI_TOOLCHAIN=nightly
#   make cargo-miri-core MIRI_CORE_FILTER=...
#   make cargo-miri-core MIRI_CORE_ARC_SWAP_FILTER=...
#   make cargo-miri-plugin MIRI_PLUGIN_FILTER=...
#   make cargo-miri-plugin MIRI_PLUGIN_MANIFEST_FILTER=...
MIRI_TOOLCHAIN ?= $(shell bash scripts/tool-version.sh miri)
MIRI_FLAGS ?= -Zmiri-disable-isolation -Zmiri-strict-provenance
MIRI_CORE_ARC_SWAP_FLAGS ?= -Zmiri-disable-isolation -Zmiri-permissive-provenance
MIRI_PLUGIN_MANIFEST_FLAGS ?= $(MIRI_FLAGS) -Zmiri-ignore-leaks
MIRI_PROPTEST_CASES ?= 4

# Default test filters target modules with `unsafe` blocks or hand-rolled
# pointer/integer code where Miri provides the most signal. Miri runs ~10-100x
# slower than native, so we narrow the default scope; pass the override above
# (or `MIRI_CORE_FILTER=`) to widen it.
MIRI_CORE_FILTER ?= -E 'test(/^(string::stack_str|nanos|uuid|hex|correctness|datetime)::/)'
# `collections::` covers AtomicMap/AtomicSet, which are backed by arc-swap.
# arc-swap runs Miri with permissive provenance, so use the same provenance
# policy for this slice while keeping strict provenance for in-tree pointer code.
MIRI_CORE_ARC_SWAP_FILTER ?= -E 'test(/^collections::/)'
MIRI_CORE_FFI_FILTER ?= ffi::cvec::tests
# `test_price_to_order_id_{comprehensive_collision_check,realistic_orderbook_prices}`
# iterate over the full price space to verify hash uniqueness. They run for
# multiple hours under the Miri interpreter and exercise no unsafe, so we skip
# them here while keeping the rest of `orderbook::` in scope.
MIRI_MODEL_FILTER ?= -E 'test(/^(types::|identifiers::|orderbook::)/) and not test(=orderbook::aggregation::tests::test_price_to_order_id_comprehensive_collision_check) and not test(=orderbook::aggregation::tests::test_price_to_order_id_realistic_orderbook_prices)'
# Keep the plug-in Miri lane focused on the ABI boundary and panic guards.
# Manifest fixtures model static cdylib storage with `Box::leak`, so that slice
# runs with leak detection disabled while the boundary tests stay strict.
MIRI_PLUGIN_FILTER ?= -E 'test(/^(boundary|panic)::/)'
MIRI_PLUGIN_MANIFEST_FILTER ?= -E 'test(/^manifest::/)'

.PHONY: check-miri-toolchain
check-miri-toolchain:
	$(Q)bash scripts/ci/check-miri-toolchain.bash

.PHONY: check-miri-installed
check-miri-installed: check-miri-toolchain
	@if ! cargo +$(MIRI_TOOLCHAIN) miri --version >/dev/null 2>&1; then \
		echo "cargo-miri is not installed for toolchain $(MIRI_TOOLCHAIN)"; \
		echo "Install with: rustup toolchain install $(MIRI_TOOLCHAIN) --component miri"; \
		exit 1; \
	fi

.PHONY: cargo-miri-core
cargo-miri-core: export RUST_BACKTRACE=1
cargo-miri-core: export PROPTEST_CASES=$(MIRI_PROPTEST_CASES)
cargo-miri-core: check-miri-installed check-nextest-installed
cargo-miri-core:  #-- Run nautilus-core library tests under Miri to detect UB
	$(info $(M) Running nautilus-core tests under Miri with strict provenance (filter: $(MIRI_CORE_FILTER))...)
	MIRIFLAGS="$(MIRI_FLAGS)" cargo +$(MIRI_TOOLCHAIN) miri nextest run -p nautilus-core --no-default-features --lib $(MIRI_CORE_FILTER)
	$(info $(M) Running nautilus-core collections tests under Miri with permissive provenance (filter: $(MIRI_CORE_ARC_SWAP_FILTER))...)
	MIRIFLAGS="$(MIRI_CORE_ARC_SWAP_FLAGS)" cargo +$(MIRI_TOOLCHAIN) miri nextest run -p nautilus-core --no-default-features --lib $(MIRI_CORE_ARC_SWAP_FILTER)
	$(info $(M) Running nautilus-core CVec FFI tests under Miri (filter: $(MIRI_CORE_FFI_FILTER))...)
	MIRIFLAGS="$(MIRI_FLAGS)" cargo +$(MIRI_TOOLCHAIN) miri test -p nautilus-core --lib --features ffi $(MIRI_CORE_FFI_FILTER)

.PHONY: cargo-miri-core-ffi
cargo-miri-core-ffi: export RUST_BACKTRACE=1
cargo-miri-core-ffi: export MIRIFLAGS=$(MIRI_FLAGS)
cargo-miri-core-ffi: check-miri-installed
cargo-miri-core-ffi:  #-- Run CVec FFI tests under Miri
	cargo +$(MIRI_TOOLCHAIN) miri test -p nautilus-core --lib --features ffi $(MIRI_CORE_FFI_FILTER)

.PHONY: cargo-miri-model
cargo-miri-model: export RUST_BACKTRACE=1
cargo-miri-model: export MIRIFLAGS=$(MIRI_FLAGS)
cargo-miri-model: export PROPTEST_CASES=$(MIRI_PROPTEST_CASES)
cargo-miri-model: check-miri-installed check-nextest-installed
cargo-miri-model:  #-- Run nautilus-model library tests under Miri to detect UB
	$(info $(M) Running nautilus-model tests under Miri (filter: $(MIRI_MODEL_FILTER))...)
	cargo +$(MIRI_TOOLCHAIN) miri nextest run -p nautilus-model --no-default-features --lib $(MIRI_MODEL_FILTER)

.PHONY: cargo-miri-plugin
cargo-miri-plugin: export RUST_BACKTRACE=1
cargo-miri-plugin: export PROPTEST_CASES=$(MIRI_PROPTEST_CASES)
cargo-miri-plugin: check-miri-installed check-nextest-installed
cargo-miri-plugin:  #-- Run nautilus-plugin boundary and manifest tests under Miri
	$(info $(M) Running nautilus-plugin library tests under Miri (filter: $(MIRI_PLUGIN_FILTER))...)
	MIRIFLAGS="$(MIRI_FLAGS)" \
		cargo +$(MIRI_TOOLCHAIN) miri nextest run \
		-p nautilus-plugin \
		--no-default-features \
		--lib \
		$(MIRI_PLUGIN_FILTER)
	$(info $(M) Running nautilus-plugin manifest tests under Miri (filter: $(MIRI_PLUGIN_MANIFEST_FILTER))...)
	MIRIFLAGS="$(MIRI_PLUGIN_MANIFEST_FLAGS)" \
		cargo +$(MIRI_TOOLCHAIN) miri nextest run \
		-p nautilus-plugin \
		--no-default-features \
		--lib \
		$(MIRI_PLUGIN_MANIFEST_FILTER)

.PHONY: cargo-miri
cargo-miri:  #-- Run Miri across the in-scope foundational and plug-in crates
	$(MAKE) cargo-miri-core
	$(MAKE) cargo-miri-model
	$(MAKE) cargo-miri-plugin

#------------------------------------------------------------------------------
# Benchmarks
#------------------------------------------------------------------------------

# List of crates whose criterion/iai benches run in the performance workflow
CI_BENCH_CRATES := nautilus-core nautilus-model nautilus-common \
	nautilus-execution nautilus-backtest nautilus-live

# CodSpeed excludes iai, iter_custom, with_filter, OS-dependent, and concurrent benchmarks
CODSPEED_BENCH_CRATES := nautilus-core nautilus-model nautilus-common nautilus-execution
CODSPEED_BENCH_TARGETS := \
	datetime stack_str identifier_comparison decimal_deserialization hash_map hex \
	to_snake_case urlencoding \
	greeks_criterion black_scholes_criterion fixed_precision_criterion \
	f64_vs_decimal_to_price_quantity money_criterion price_criterion quantity_criterion \
	expressions_criterion order_fills_criterion position_replay_criterion \
	cache_orders cache_query_sets cache_xrate client_order_id order_list_id position_id matching \
	msgbus mstr throttler matching_core
CODSPEED_BENCH_ARGS := $(addprefix --package ,$(CODSPEED_BENCH_CRATES)) \
	$(addprefix --bench ,$(CODSPEED_BENCH_TARGETS))

# NOTE:
# - We invoke `cargo bench` *once per crate* to avoid the well-known
#   "mixed panic strategy" linker error that appears when crates which specify
#   different `panic` strategies (e.g. `abort` for cdylib/staticlib targets vs
#   `unwind` for Criterion) are linked into the *same* benchmark binary.
# - Cargo will still reuse compiled artifacts between iterations, so the cost
#   of the extra invocations is marginal while the linker remains happy.

.PHONY: cargo-ci-benches
cargo-ci-benches:  #-- Run Rust benches for the crates included in the CI performance workflow
	@for crate in $(CI_BENCH_CRATES); do \
	  echo "Running benches for $$crate"; \
	  cargo bench -p $$crate --profile bench --benches --no-fail-fast; \
	done

.PHONY: cargo-codspeed-build
cargo-codspeed-build:  #-- Build the selected Rust benchmarks for CodSpeed CPU simulation
	cargo codspeed build --locked --measurement-mode simulation $(CODSPEED_BENCH_ARGS)

.PHONY: cargo-codspeed-run
cargo-codspeed-run:  #-- Run the selected Rust benchmarks previously built for CodSpeed
	cargo codspeed run --measurement-mode simulation $(CODSPEED_BENCH_ARGS)

#== Docker

.PHONY: docker-build
docker-build: clean  #-- Build Docker image for NautilusTrader
	bash scripts/ci/docker-pull-retry.sh $(IMAGE_FULL) || bash scripts/ci/docker-pull-retry.sh $(IMAGE):nightly || true
	bash scripts/ci/docker-pull-retry.sh --from-dockerfile .docker/nautilus_trader.dockerfile
	docker build -f .docker/nautilus_trader.dockerfile --platform linux/x86_64 -t $(IMAGE_FULL) .

.PHONY: docker-build-force
docker-build-force:  #-- Force rebuild Docker image without cache
	bash scripts/ci/docker-pull-retry.sh --from-dockerfile .docker/nautilus_trader.dockerfile
	docker build --no-cache -f .docker/nautilus_trader.dockerfile -t $(IMAGE_FULL) .

.PHONY: docker-push
docker-push:  #-- Push Docker image to registry
	docker push $(IMAGE_FULL)

.PHONY: docker-build-jupyter
docker-build-jupyter:  #-- Build JupyterLab Docker image
	docker build -f .docker/jupyterlab.dockerfile --platform linux/x86_64 -t $(IMAGE):jupyter .

.PHONY: docker-push-jupyter
docker-push-jupyter:  #-- Push JupyterLab Docker image to registry
	docker push $(IMAGE):jupyter

.PHONY: init-services
init-services:  #-- Initialize development services eg. for integration tests (start containers and setup database)
	$(info $(M) Initializing development services...)
	@$(MAKE) start-services
	@printf "$(PURPLE)Waiting for PostgreSQL to be ready...$(RESET)\n"
	@sleep 10
	@$(MAKE) init-db

.PHONY: start-services
start-services:  #-- Start development services (without reinitializing database)
	$(info $(M) Starting development services...)
	bash scripts/ci/docker-pull-retry.sh public.ecr.aws/docker/library/postgres
	bash scripts/ci/docker-pull-retry.sh dpage/pgadmin4
	bash scripts/ci/docker-pull-retry.sh public.ecr.aws/docker/library/redis
	docker compose -f .docker/docker-compose.yml up -d

.PHONY: stop-services
stop-services:  #-- Stop development services (preserves data)
	$(info $(M) Stopping development services...)
	docker compose -f .docker/docker-compose.yml down

.PHONY: purge-services
purge-services:  #-- Purge all development services (stop containers and remove volumes)
	$(info $(M) Purging integration test services...)
	docker compose -f .docker/docker-compose.yml down -v

.PHONY: init-db
init-db:  #-- Initialize PostgreSQL database schema
	$(info $(M) Initializing PostgreSQL database schema...)
	cat schema/sql/types.sql schema/sql/tables.sql schema/sql/functions.sql schema/sql/partitions.sql | docker exec -i nautilus-database psql -U nautilus -d nautilus

#== Python Testing

.PHONY: pytest-collect-fast
pytest-collect-fast:  #-- Collect Python tests against the existing extension
	@if [ -z "$(PYTHON_EXTENSION_PATH)" ]; then \
		printf "$(YELLOW)Skipping Python test collection: run \`make build-debug\` first$(RESET)\n"; \
	else \
		printf "$(M) Collecting Python tests without rebuilding...\n"; \
		cd python && VIRTUAL_ENV= uv run --no-sync pytest tests/ --collect-only -q; \
	fi

.PHONY: pytest
pytest: build-debug  #-- Run Python tests
	$(info $(M) Running Python tests...)
	$Q cd python && VIRTUAL_ENV= uv run --no-sync pytest -qq -rfE tests/ --ignore=tests/unit/test_live_node.py
	$Q cd python && VIRTUAL_ENV= uv run --no-sync pytest -qq -rfE tests/unit/test_live_node.py

.PHONY: pytest-doctest
pytest-doctest: build-debug  #-- Run supported Python doctests
	$(info $(M) Running supported Python doctests...)
	$Q bash scripts/ci/test-python-doctests.bash "$(CURDIR)/python"

.PHONY: pytest-memray
pytest-memray: build-debug  #-- Run Python memory leak tests with Memray
	$(info $(M) Running Python memory leak tests...)
	$Q cd python && VIRTUAL_ENV= uv run --no-sync pytest -qq -rfE memray_tests/

.PHONY: ty
ty: build-debug  #-- Type-check Python examples
	$(info $(M) Type-checking Python examples...)
	$Q bash scripts/ci/check-python-types.bash python examples

#== CLI Tools

.PHONY: install-cli
install-cli:  #-- Install Nautilus CLI tool from source
	cargo install --path crates/cli --bin nautilus --locked --force

#== Internal

.PHONY: help
help:  #-- Show this help message and exit
	@printf "NautilusTrader Makefile\n\n"
	@printf "$(GRAY)Requires GNU Make. Windows users can install it via MSYS2 or WSL.$(RESET)\n\n"
	@printf "$(GREEN)Usage:$(RESET) make $(CYAN)<target>$(RESET)\n\n"
	@printf "$(GRAY)Tips: Use $(CYAN)make <target> V=1$(GRAY) for verbose output$(RESET)\n"
	@printf "$(GRAY)      Use $(CYAN)make <target> NEXTEST_VERBOSE=true$(GRAY) for verbose Nextest output$(RESET)\n\n"

	@printf "⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣀⣀⣀⡤⠤⠤⠤⠤⠤⠤⠤⢤⡀⠀⠀⠀⠀⠀⠀\n"
	@printf "⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣠⠤⠖⠚⠉⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣸⠁⠀⠀⠀⠀⠀⠀\n"
	@printf "⠀⠀⠀⠀⠀⠀⢀⣠⠖⠋⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣀⡴⠚⠁⠀⠀⠀⠀⠀⠀⠀\n"
	@printf "⠀⠀⠀⠀⢀⡴⠋⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⡞⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀\n"
	@printf "⠀⠀⠀⣠⠏⠀⠀⠀⠀⠀⠀⠀⠀⢀⣠⠤⠖⠒⠒⠒⠒⠋⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀\n"
	@printf "⠀⠀⠀⡇⠀⠀⠀⠀⠀⠀⠀⢀⡖⠋⣠⢴⢪⠞⣩⢟⡭⠵⢤⣤⣀⠀⠀⠀⠀⠀⠀⠀⠀⠀\n"
	@printf "⠀⠀⣦⡙⠦⣄⡀⠀⢀⣠⠴⠋⢠⡐⣇⢸⡘⢦⡇⣏⡴⣋⡭⠖⠮⢥⡀⠀⠀⠀⠀⠀⠀⠀\n"
	@printf "⠀⠀⢸⡉⠓⠦⠭⠭⠭⠴⣺⠃⠸⣷⢬⣓⣛⠒⠩⣌⢡⡷⢒⣫⠭⣝⡛⠆⠀⠀⠀⠀⠀⠀\n"
	@printf "⠀⠀⠀⠙⠦⢤⣀⣠⠤⠞⣡⠞⡆⠺⣭⣭⡷⢇⣷⡻⠡⠾⣛⣒⠦⢤⡙⡆⠀⠀⠀⠀⠀⠀\n"
	@printf "⠀⠀⠀⠈⠳⣖⠒⠒⠒⠋⠁⣠⠇⡼⢦⠀⣌⡉⢥⣄⣛⡻⢥⡈⠉⢳⡙⠇⠀⠀⠀⠀⠀⠀\n"
	@printf "⠀⠀⠀⠀⠀⠈⠙⣒⣒⣒⣋⡥⠞⠁⣸⠃⡇⠙⢦⢹⠀⠙⡆⢳⢀⡴⠃⠀⠀⠀⠀⠀⠀⠀\n"
	@printf "⠀⠀⠀⠀⠀⠀⠀⠈⠙⠒⠦⠤⠴⣚⣡⠞⠁⣠⠏⡼⢀⣠⠇⠞⠋⠀⠀⠀⠀⠀⠀⠀⠀⠀\n"
	@printf "⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠉⠉⠉⠉⠉⠑⠚⠋⠉⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀\n"

	@awk '\
	BEGIN { \
		FS = ":.*#--"; \
		target_maxlen = 0; \
		GREEN = "\033[0;32m"; \
		CYAN = "\033[0;36m"; \
		RESET = "\033[0m"; \
	} \
	/^[$$()% a-zA-Z0-9_-]+:.*?#--/ { \
		if (length($$1) > target_maxlen) target_maxlen = length($$1); \
		targets[NR] = $$1; descriptions[NR] = $$2; \
	} \
	/^#==/ { \
		groups[NR] = substr($$0, 5); \
	} \
	END { \
		for (i = 1; i <= NR; i++) { \
			if (groups[i]) { \
				printf "\n" GREEN "%s:" RESET "\n", groups[i]; \
			} else if (targets[i]) { \
				printf "  " CYAN "%-*s" RESET " %s\n", target_maxlen, targets[i], descriptions[i]; \
			} \
		} \
	}' $(MAKEFILE_LIST)
