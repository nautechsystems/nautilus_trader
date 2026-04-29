# Variables
# -----------------------------------------------------------------------------
PROJECT?=nautechsystems/nautilus_trader
REGISTRY?=ghcr.io/
IMAGE?=$(REGISTRY)$(PROJECT)
GIT_TAG:=$(shell git rev-parse --abbrev-ref HEAD)
IMAGE_FULL?=$(IMAGE):$(GIT_TAG)

# Tool versions from Cargo.toml [workspace.metadata.tools]
CARGO_AUDIT_VERSION := $(shell bash scripts/cargo-tool-version.sh cargo-audit)
CARGO_DENY_VERSION := $(shell bash scripts/cargo-tool-version.sh cargo-deny)
CARGO_EDIT_VERSION := $(shell bash scripts/cargo-tool-version.sh cargo-edit)
CARGO_LLVM_COV_VERSION := $(shell bash scripts/cargo-tool-version.sh cargo-llvm-cov)
CARGO_MACHETE_VERSION := $(shell bash scripts/cargo-tool-version.sh cargo-machete)
CARGO_NEXTEST_VERSION := $(shell bash scripts/cargo-tool-version.sh cargo-nextest)
CARGO_VET_VERSION := $(shell bash scripts/cargo-tool-version.sh cargo-vet)
LYCHEE_VERSION := $(shell bash scripts/cargo-tool-version.sh lychee)
# Tool versions from tools.toml
PREK_VERSION := $(shell bash scripts/tool-version.sh prek)
UV_VERSION := $(shell bash scripts/uv-version.sh)

V = 0  # 0 / 1 - verbose mode
Q = $(if $(filter 1,$V),,@) # Quiet mode, suppress command output
M = $(shell printf "\033[0;34m>\033[0m") # Message prefix for commands

# Verbose options for specific targets (defaults to true, can be overridden)
VERBOSE ?= true

# TARGET_DIR controls where cargo places build artifacts.
# Can be overridden to use a separate directory: make build-debug TARGET_DIR=target-python
TARGET_DIR ?= target

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
BASE_FEATURES := arrow,ffi,python,high-precision,streaming,defi
else
BASE_FEATURES := arrow,ffi,python,high-precision,streaming
endif

# Combine base features with extra features
ifneq ($(strip $(EXTRA_FEATURES)),)
CARGO_FEATURES := $(BASE_FEATURES),$(EXTRA_FEATURES)
else
CARGO_FEATURES := $(BASE_FEATURES)
endif

# Core crates (excludes adapters/*, nautilus-pyo3, nautilus-cli)
CORE_CRATES := nautilus-analysis nautilus-backtest nautilus-common nautilus-core \
    nautilus-cryptography nautilus-data nautilus-execution nautilus-indicators \
    nautilus-infrastructure nautilus-live nautilus-model nautilus-network \
    nautilus-persistence nautilus-portfolio nautilus-risk nautilus-serialization \
    nautilus-system nautilus-testkit nautilus-trading

# Adapter crates (crates/adapters/*)
ADAPTER_CRATES := nautilus-architect-ax nautilus-betfair nautilus-binance \
    nautilus-bitmex nautilus-blockchain nautilus-bybit nautilus-databento \
    nautilus-deribit nautilus-dydx nautilus-hyperliquid nautilus-kraken \
    nautilus-okx nautilus-polymarket nautilus-sandbox nautilus-tardis

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

.PHONY: install-deps
install-deps:  #-- Install Python dependencies only (no package build)
	$(info $(M) Installing Python dependencies...)
	$Q uv sync --active --all-groups --all-extras --inexact --no-install-package nautilus_trader

.PHONY: install
install: install-deps
install: export BUILD_MODE=release
install:  #-- Install in release mode with all dependencies and extras
	$(info $(M) Installing NautilusTrader in release mode...)
	$Q uv sync --active --all-groups --all-extras --inexact

.PHONY: install-debug
install-debug: install-deps
install-debug: export BUILD_MODE=debug
install-debug:  #-- Install in debug mode for development
	$(info $(M) Installing NautilusTrader in debug mode...)
	$Q uv sync --active --all-groups --all-extras --inexact

#== Build

.PHONY: build
build: install-deps
build: export BUILD_MODE=release
build: export CARGO_TARGET_DIR=$(TARGET_DIR)
build:  #-- Build the package in release mode
	uv run --active --no-sync build.py

.PHONY: build-debug
build-debug: install-deps
build-debug: export BUILD_MODE=debug
build-debug: export CARGO_TARGET_DIR=$(TARGET_DIR)
build-debug:  #-- Build the package in debug mode (recommended for development)
ifeq ($(VERBOSE),true)
	$(info $(M) Building in debug mode with verbose output...)
	uv run --active --no-sync build.py
else
	$(info $(M) Building in debug mode (errors will still be shown)...)
	uv run --active --no-sync build.py 2>&1 | grep -E "(Error|error|ERROR|Failed|failed|FAILED|Warning|warning|WARNING|Build completed|Build time:|Traceback)" || true
endif

.PHONY: build-debug-pyo3
build-debug-pyo3: export BUILD_MODE=debug-pyo3
build-debug-pyo3: export CARGO_TARGET_DIR=$(TARGET_DIR)
build-debug-pyo3:  #-- Build the package with PyO3 debug symbols (for debugging Rust code)
ifeq ($(VERBOSE),true)
	$(info $(M) Building in debug mode with PyO3 debug symbols...)
	uv run --active --no-sync build.py
else
	$(info $(M) Building in debug mode with PyO3 debug symbols (errors will still be shown)...)
	uv run --active --no-sync build.py 2>&1 | grep -E "(Error|error|ERROR|Failed|failed|FAILED|Warning|warning|WARNING|Build completed|Build time:|Traceback)" || true
endif

.PHONY: build-wheel
build-wheel: export BUILD_MODE=release
build-wheel:  #-- Build wheel distribution in release mode
	uv build --wheel

.PHONY: build-wheel-debug
build-wheel-debug: export BUILD_MODE=debug
build-wheel-debug:  #-- Build wheel distribution in debug mode
	uv build --wheel

.PHONY: build-dry-run
build-dry-run: export DRY_RUN=true
build-dry-run:  #-- Show build commands without executing them
	uv run --active --no-sync build.py

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
	$Q rm -rf dist target target-v2 2>/dev/null || true

.PHONY: clean-build-artifacts
clean-build-artifacts:  #-- Clean compiled artifacts (.so, .dll, .pyc, .c files)
	@echo "Cleaning build artifacts..."
	# Clean Rust build artifacts (keep final libraries)
	find target target-v2 -name "*.rlib" -delete 2>/dev/null || true
	find target target-v2 -name "*.rmeta" -delete 2>/dev/null || true
	rm -rf target/*/build target/*/deps target-v2/*/build target-v2/*/deps 2>/dev/null || true
	# Clean Python build artifacts
	find . -type d -name "__pycache__" -not -path "./.venv*" -exec rm -rf {} + 2>/dev/null || true
	find . -type f -name "*.c" -not -path "./.venv*" -not -path "./target/*" -not -path "./target-v2/*" -exec rm -f {} + 2>/dev/null || true
	find . -type f -a \( -name "*.pyc" -o -name "*.pyo" \) -not -path "./.venv*" -exec rm -f {} + 2>/dev/null || true
	find . -type f -a \( -name "*.so" -o -name "*.dll" -o -name "*.dylib" \) -not -path "./.venv*" -exec rm -f {} + 2>/dev/null || true
	rm -rf build/ cython_debug/ 2>/dev/null || true
	# Clean test artifacts
	rm -rf .coverage .benchmarks 2>/dev/null || true

.PHONY: clean-caches
clean-caches:  #-- Clean pytest, mypy, ruff, uv, and cargo caches
	rm -rf .pytest_cache .mypy_cache .ruff_cache 2>/dev/null || true
	-uv cache prune --force
	-cargo clean --workspace
	-CARGO_TARGET_DIR=target-v2 cargo clean --workspace

.PHONY: distclean
distclean: clean  #-- Nuclear clean - remove all untracked files (requires FORCE=1)
	@if [ "$$FORCE" != "1" ]; then \
		echo "Pass FORCE=1 to really nuke"; \
		exit 1; \
	fi
	@echo "WARNING: removing all untracked files (git clean -fxd)..."
	git clean -fxd -e tests/test_data/large/ -e .venv

#== Code Quality

.PHONY: format
format:  #-- Format Rust (with nightly) and Python code
	cargo +nightly fmt
	uv run --active --no-sync ruff format .

.PHONY: pre-commit
pre-commit:  #-- Run all pre-commit hooks on all files
	prek run --all-files

# The check-code target uses CARGO_FEATURES which is controlled by the HYPERSYNC flag.
# By default, hypersync is excluded to speed up checks. Override with: make check-code HYPERSYNC=true
.PHONY: check-code
check-code:  #-- Run clippy on lib/test targets and ruff --fix (use HYPERSYNC=true to include hypersync feature)
	$(info $(M) Running code quality checks...)
	@cargo clippy --workspace --lib --tests --features "$(CARGO_FEATURES)" --profile nextest -- -D warnings
	@uv run --active --no-sync ruff check . --fix
	@printf "$(GREEN)Checks passed$(RESET)\n"

.PHONY: check-all-targets
check-all-targets:  #-- Run clippy on all targets including bins and examples (nightly)
	$(info $(M) Running full clippy on all targets...)
	@cargo clippy --workspace --all-targets --features "$(CARGO_FEATURES),examples" --profile nextest -- -D warnings
	@printf "$(GREEN)All-targets check passed$(RESET)\n"

.PHONY: pre-flight
pre-flight: export CARGO_TARGET_DIR=$(TARGET_DIR)
pre-flight:  #-- Run pre-flight checks (format, check-code, cargo-test, build-debug, pytest)
	$(info $(M) Running pre-flight checks...)
	@if ! git diff --quiet; then \
		printf "$(RED)ERROR: You have unstaged changes$(RESET)\n"; \
		printf "$(YELLOW)Stage your changes first:$(RESET) git add .\n"; \
		exit 1; \
	fi
	@$(MAKE) --no-print-directory install-deps
	@$(MAKE) --no-print-directory format
	@$(MAKE) --no-print-directory check-code EXTRA_FEATURES="capnp,hypersync"
	@$(MAKE) --no-print-directory cargo-test-extras
	@$(MAKE) --no-print-directory build-debug
	@$(MAKE) --no-print-directory pytest
	@printf "$(GREEN)All pre-flight checks passed$(RESET)\n"

.PHONY: ruff
ruff:  #-- Run ruff linter with automatic fixes
	uv run --active --no-sync ruff check . --fix

.PHONY: clippy
clippy:  #-- Run clippy linter (check only, workspace lints)
	cargo clippy --all-targets --all-features -- -D warnings

.PHONY: clippy-fix
clippy-fix:  #-- Run clippy linter with automatic fixes (workspace lints)
	cargo clippy --fix --all-targets --all-features --allow-dirty --allow-staged -- -D warnings

.PHONY: clippy-fix-nightly
clippy-fix-nightly:  #-- Run clippy linter with nightly toolchain and automatic fixes (workspace lints + additional strictness)
	cargo +nightly clippy --fix --all-targets --all-features --allow-dirty --allow-staged -- -D warnings

.PHONY: clippy-pedantic-crate-%
clippy-pedantic-crate-%:  #-- Run clippy linter for a specific Rust crate (usage: make clippy-crate-<crate_name>)
	cargo clippy --all-targets --all-features -p $* -- -D warnings \
		-W clippy::todo \
		-W clippy::unwrap_used \
		-W clippy::expect_used

#== Dependencies

.PHONY: outdated
outdated: check-edit-installed  #-- Check for outdated dependencies
	cargo upgrade --dry-run --incompatible
	uv tree --outdated --depth 1 --all-groups
	@printf "\n$(CYAN)Checking tool versions...$(RESET)\n"
	@outdated_count=0; \
	for tool in cargo-audit:$(CARGO_AUDIT_VERSION) cargo-deny:$(CARGO_DENY_VERSION) cargo-edit:$(CARGO_EDIT_VERSION) cargo-llvm-cov:$(CARGO_LLVM_COV_VERSION) cargo-machete:$(CARGO_MACHETE_VERSION) cargo-nextest:$(CARGO_NEXTEST_VERSION) cargo-vet:$(CARGO_VET_VERSION) lychee:$(LYCHEE_VERSION); do \
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
	uv lock --upgrade

.PHONY: update-uv
update-uv:  #-- Install or upgrade uv to the version pinned in pyproject.toml
	$(info $(M) Ensuring uv $(UV_VERSION) is installed...)
	@if [ "$$(uv --version 2>/dev/null | awk '{print $$2}')" = "$(UV_VERSION)" ]; then \
		printf "$(GREEN)uv $(UV_VERSION) already installed$(RESET)\n"; \
	else \
		curl -LsSf https://astral.sh/uv/$(UV_VERSION)/install.sh | sh; \
	fi

.PHONY: install-tools
install-tools: check-binstall-installed update-uv  #-- Install required development tools (pinned versions from Cargo.toml, tools.toml, pyproject.toml)
	cargo install cargo-deny --version $(CARGO_DENY_VERSION) --locked \
	&& cargo install cargo-edit --version $(CARGO_EDIT_VERSION) --locked \
	&& cargo install cargo-machete --version $(CARGO_MACHETE_VERSION) --locked \
	&& cargo install cargo-nextest --version $(CARGO_NEXTEST_VERSION) --locked \
	&& cargo install cargo-llvm-cov --version $(CARGO_LLVM_COV_VERSION) --locked \
	&& cargo install cargo-audit --version $(CARGO_AUDIT_VERSION) --locked \
	&& cargo install cargo-vet --version $(CARGO_VET_VERSION) --locked \
	&& cargo install lychee --version $(LYCHEE_VERSION) --locked \
	&& cargo binstall prek --version $(PREK_VERSION) --no-confirm --locked \
	&& bash scripts/install-osv-scanner.sh

#== Security

.PHONY: security-audit
security-audit: check-audit-installed check-deny-installed check-vet-installed check-osv-scanner-installed  #-- Run comprehensive security audit (cargo-audit, cargo-deny, cargo-vet, pip-audit, osv-scanner)
	$(info $(M) Running security audit...)
	@printf "$(CYAN)Running cargo audit...$(RESET)\n"
	cargo audit --color never
	@printf "\n$(CYAN)Running cargo deny (advisories, licenses, sources, bans)...$(RESET)\n"
	cargo deny --all-features check advisories licenses sources bans
	@printf "\n$(CYAN)Running cargo vet (supply chain audit)...$(RESET)\n"
	cargo vet --locked
	@printf "\n$(CYAN)Running pip-audit (Python dependencies)...$(RESET)\n"
	uv export --no-hashes --frozen | uv run --no-project --with pip-audit -- pip-audit --disable-pip --no-deps -r /dev/stdin
	@printf "\n$(CYAN)Running osv-scanner (Cargo.lock + uv.lock + python/uv.lock)...$(RESET)\n"
	osv-scanner --config=osv-scanner.toml --lockfile=Cargo.lock --lockfile=uv.lock --lockfile=python/uv.lock

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
docs-python: export BUILD_MODE=debug
docs-python:  #-- Build Python documentation with Sphinx
	uv run --active --no-sync sphinx-build -M html ./docs/api_reference ./api_reference

.PHONY: docs-rust
docs-rust: export RUSTDOCFLAGS=--enable-index-page -Zunstable-options
docs-rust:  #-- Build Rust documentation with cargo doc
	cargo +nightly doc --all-features --no-deps --workspace

.PHONY: docsrs-check
docsrs-check: export DOCS_RS=1
docsrs-check: export RUSTDOCFLAGS=--cfg docsrs -D warnings
docsrs-check: check-hack-installed #-- Check documentation builds for docs.rs compatibility
	cargo +nightly hack --workspace doc --no-deps --all-features

.PHONY: docs-check-links
docs-check-links:  #-- Check for broken links in documentation (periodic audit)
	$(info $(M) Checking documentation links...)
	@lychee \
		--verbose \
		--no-progress \
		--exclude-all-private \
		--max-retries 3 \
		--retry-wait-time 5 \
		--timeout 30 \
		--max-concurrency 10 \
		--accept "100..=103,200..=299,429,502..=504" \
		--include-fragments \
		--fallback-extensions md,py,html \
		--exclude-path .venv \
		--exclude-path target \
		--exclude-path docs/python-api-latest \
		--exclude "file://.*/python-api-latest/.*" \
		--exclude-file .lycheeignore \
		"**/*.md" "docs/**/*.py"
	@printf "$(GREEN)Link check passed$(RESET)\n"

#== Rust Development

.PHONY: cargo-build
cargo-build:  #-- Build Rust crates in release mode
	cargo build --release --all-features

.PHONY: cargo-update
cargo-update:  #-- Update Rust dependencies (versions from Cargo.toml)
	cargo update

.PHONY: cargo-check
cargo-check:  #-- Check Rust code without building
	cargo check --workspace --all-features

# Security tool checks
.PHONY: check-audit-installed
check-audit-installed:  #-- Verify cargo-audit is installed
	@if ! cargo audit --version >/dev/null 2>&1; then \
		echo "cargo-audit is not installed. You can install it using 'cargo install cargo-audit'"; \
		exit 1; \
	fi

.PHONY: check-deny-installed
check-deny-installed:  #-- Verify cargo-deny is installed
	@if ! cargo deny --version >/dev/null 2>&1; then \
		echo "cargo-deny is not installed. You can install it using 'cargo install cargo-deny'"; \
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

.PHONY: check-osv-scanner-installed
check-osv-scanner-installed:  #-- Verify osv-scanner is installed and version matches tools.toml
	@if ! osv-scanner --version >/dev/null 2>&1; then \
		echo "osv-scanner is not installed. See https://google.github.io/osv-scanner/installation/"; \
		exit 1; \
	fi
	@EXPECTED=$$(bash scripts/tool-version.sh osv-scanner); \
	INSTALLED=$$(osv-scanner --version 2>&1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1); \
	if [ "$$INSTALLED" != "$$EXPECTED" ]; then \
		printf "$(YELLOW)osv-scanner version mismatch: installed %s, expected %s (from tools.toml)$(RESET)\n" "$$INSTALLED" "$$EXPECTED"; \
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

.PHONY: check-features
check-features: check-hack-installed  #-- Verify crate feature combinations compile correctly
	cargo hack --workspace check --each-feature --all-targets

.PHONY: check-capnp-schemas  #-- Verify Cap'n Proto schemas are up-to-date
check-capnp-schemas:
	$(info $(M) Checking if Cap'n Proto schemas are up-to-date...)
	@if ! command -v capnp > /dev/null 2>&1; then \
		echo "$(YELLOW)⚠ capnp not installed, skipping schema check$(RESET)"; \
	elif ! CAPNP_CHECK=1 bash scripts/regen_capnp.sh; then \
		echo "$(RED)Error: Cap'n Proto regeneration failed$(RESET)"; \
		echo "Run manually to see errors: ./scripts/regen_capnp.sh"; \
		exit 1; \
	else \
		DIFF_OUTPUT="$$(git diff -I\"ENCODED_NODE\" -- crates/serialization/generated/capnp)"; \
		if [ -n "$$DIFF_OUTPUT" ]; then \
			echo "$(RED)Error: Cap'n Proto generated files are out of date$(RESET)"; \
			echo "Please run: ./scripts/regen_capnp.sh"; \
			echo "Or: make regen-capnp"; \
			exit 1; \
		else \
			echo "$(GREEN)✓ Cap'n Proto schemas are up-to-date$(RESET)"; \
		fi; \
	fi

.PHONY: regen-capnp  #-- Regenerate Cap'n Proto schema files
regen-capnp:
	$(info $(M) Regenerating Cap'n Proto schemas...)
	@bash scripts/regen_capnp.sh

#== Rust Testing

.PHONY: cargo-test
cargo-test: export RUST_BACKTRACE=1
cargo-test: check-nextest-installed
cargo-test:  #-- Run all Rust tests (use EXTRA_FEATURES="feature1 feature2" or HYPERSYNC=true)
ifeq ($(VERBOSE),true)
	$(info $(M) Running Rust tests with verbose output...)
	cargo nextest run --workspace --lib --tests --features "$(CARGO_FEATURES)" $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile nextest --verbose
else
	$(info $(M) Running Rust tests (showing summary and failures only)...)
	cargo nextest run --workspace --lib --tests --features "$(CARGO_FEATURES)" $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile nextest --status-level fail --final-status-level flaky
endif

.PHONY: cargo-test-extras
cargo-test-extras:  #-- Run all Rust tests with capnp and hypersync features (convenience shortcut)
	$(MAKE) cargo-test EXTRA_FEATURES="capnp,hypersync"

# Both core and adapter targets use identical --workspace --features flags so
# cargo sees the same feature union and does not recompile between runs.
# The -E filterset selects which tests to execute.
CORE_FILTERSET := $(subst $(eval ) , + ,$(foreach crate,$(CORE_CRATES),package($(crate))))
ADAPTER_FILTERSET := $(subst $(eval ) , + ,$(foreach crate,$(ADAPTER_CRATES),package($(crate))))

.PHONY: cargo-test-core-local
cargo-test-core-local: export RUST_BACKTRACE=1
cargo-test-core-local: check-nextest-installed
cargo-test-core-local:  #-- Run Rust tests for core crates only with direct package selection (fast local compile)
ifeq ($(VERBOSE),true)
	$(info $(M) Running Rust tests for core crates with direct package selection...)
	cargo nextest run $(foreach crate,$(CORE_CRATES),-p $(crate)) --lib --tests --features "$(CARGO_FEATURES)" $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile nextest --verbose
else
	$(info $(M) Running Rust tests for core crates with direct package selection (showing summary and failures only)...)
	cargo nextest run $(foreach crate,$(CORE_CRATES),-p $(crate)) --lib --tests --features "$(CARGO_FEATURES)" $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile nextest --status-level fail --final-status-level flaky
endif

.PHONY: cargo-test-core
cargo-test-core: export RUST_BACKTRACE=1
cargo-test-core: check-nextest-installed
cargo-test-core:  #-- Run Rust tests for core crates only (excludes adapters)
ifeq ($(VERBOSE),true)
	$(info $(M) Running Rust tests for core crates...)
	cargo nextest run --workspace --lib --tests --features "$(CARGO_FEATURES)" -E '$(CORE_FILTERSET)' $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile nextest --verbose
else
	$(info $(M) Running Rust tests for core crates (showing summary and failures only)...)
	cargo nextest run --workspace --lib --tests --features "$(CARGO_FEATURES)" -E '$(CORE_FILTERSET)' $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile nextest --status-level fail --final-status-level flaky
endif

.PHONY: cargo-test-adapters
cargo-test-adapters: export RUST_BACKTRACE=1
cargo-test-adapters: check-nextest-installed
cargo-test-adapters:  #-- Run Rust tests for adapter crates only
ifeq ($(VERBOSE),true)
	$(info $(M) Running Rust tests for adapter crates...)
	cargo nextest run --workspace --lib --tests --features "$(CARGO_FEATURES)" -E '$(ADAPTER_FILTERSET)' $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile nextest --verbose
else
	$(info $(M) Running Rust tests for adapter crates (showing summary and failures only)...)
	cargo nextest run --workspace --lib --tests --features "$(CARGO_FEATURES)" -E '$(ADAPTER_FILTERSET)' $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile nextest --status-level fail --final-status-level flaky
endif

# DST simulation smoke test. Compiles the in-scope crates under cfg(madsim)
# and runs every test that is sim-compatible today: all of nautilus-common,
# nautilus-network, and nautilus-execution (transport-bound tests are gated
# out at the source), plus the cross-crate seam pinning tests in nautilus-core.
# Each leg runs with the standard fixed-precision build first, then again
# under `high-precision` for the crates that consume `nautilus-model` types,
# so the seam-routed code paths are exercised under both `QuantityRaw` /
# `PriceRaw` widths (u64 vs u128). See docs/concepts/dst.md for the full
# DST scope.
.PHONY: cargo-test-sim
cargo-test-sim: export RUST_BACKTRACE=1
cargo-test-sim: export RUSTFLAGS=--cfg madsim
cargo-test-sim: check-nextest-installed
cargo-test-sim:  #-- Run DST simulation smoke tests (cfg madsim + simulation feature)
	$(info $(M) Building in-scope crates under simulation (compile gate)...)
	cargo build -p nautilus-common -p nautilus-core -p nautilus-network -p nautilus-execution --tests --lib --features simulation
	$(info $(M) Running nautilus-common tests under simulation...)
	cargo nextest run -p nautilus-common --features simulation $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile nextest --status-level fail --final-status-level flaky
	$(info $(M) Running nautilus-common tests under simulation + high-precision...)
	cargo nextest run -p nautilus-common --features "simulation,high-precision" $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile nextest --status-level fail --final-status-level flaky
	$(info $(M) Running nautilus-network tests under simulation...)
	cargo nextest run -p nautilus-network --features simulation $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile nextest --status-level fail --final-status-level flaky
	$(info $(M) Running nautilus-execution tests under simulation...)
	cargo nextest run -p nautilus-execution --features simulation $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile nextest --status-level fail --final-status-level flaky
	$(info $(M) Running nautilus-execution tests under simulation + high-precision...)
	cargo nextest run -p nautilus-execution --features "simulation,high-precision" $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile nextest --status-level fail --final-status-level flaky
	$(info $(M) Running nautilus-core DST seam pinning tests under simulation...)
	cargo nextest run -p nautilus-core --features simulation -E 'test(~virtual_time)' $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile nextest --status-level fail --final-status-level flaky

.PHONY: cargo-test-core-debug
cargo-test-core-debug: export RUST_BACKTRACE=1
cargo-test-core-debug: check-nextest-installed
cargo-test-core-debug:  #-- Run Rust tests for core crates (debug profile)
	cargo nextest run --workspace --lib --tests --features "$(CARGO_FEATURES)" -E '$(CORE_FILTERSET)' $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE)

.PHONY: cargo-test-core-local-debug
cargo-test-core-local-debug: export RUST_BACKTRACE=1
cargo-test-core-local-debug: check-nextest-installed
cargo-test-core-local-debug:  #-- Run Rust tests for core crates with direct package selection (debug profile)
	cargo nextest run $(foreach crate,$(CORE_CRATES),-p $(crate)) --lib --tests --features "$(CARGO_FEATURES)" $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE)

.PHONY: cargo-test-lib
cargo-test-lib: export RUST_BACKTRACE=1
cargo-test-lib: check-nextest-installed
cargo-test-lib:  #-- Run Rust library tests only with high precision
	cargo nextest run --lib --workspace --no-default-features --features "ffi,python,high-precision,streaming,defi,stubs" $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile nextest

.PHONY: cargo-test-standard-precision
cargo-test-standard-precision: export RUST_BACKTRACE=1
cargo-test-standard-precision: check-nextest-installed
cargo-test-standard-precision:  #-- Run Rust tests with standard precision (debug profile)
	cargo nextest run --workspace --lib --tests --features "ffi,python" $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE)

.PHONY: cargo-test-debug
cargo-test-debug: export RUST_BACKTRACE=1
cargo-test-debug: check-nextest-installed
cargo-test-debug:  #-- Run Rust tests with high precision (debug profile)
	cargo nextest run --workspace --lib --tests --features "ffi,python,high-precision,streaming,defi" $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE)

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
	cargo nextest run --lib $(FAIL_FAST_FLAG) --profile $(NEXTEST_PROFILE) --cargo-profile nextest -p $* --features "$$(./scripts/crate-test-features.sh $*)"

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

#------------------------------------------------------------------------------
# Benchmarks
#------------------------------------------------------------------------------

# List of crates whose criterion/iai benches run in the performance workflow
CI_BENCH_CRATES := nautilus-core nautilus-model nautilus-common nautilus-live

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

#== Docker

.PHONY: docker-build
docker-build: clean  #-- Build Docker image for NautilusTrader
	docker pull $(IMAGE_FULL) || docker pull $(IMAGE):nightly || true
	docker build -f .docker/nautilus_trader.dockerfile --platform linux/x86_64 -t $(IMAGE_FULL) .

.PHONY: docker-build-force
docker-build-force:  #-- Force rebuild Docker image without cache
	docker build --no-cache -f .docker/nautilus_trader.dockerfile -t $(IMAGE_FULL) .

.PHONY: docker-push
docker-push:  #-- Push Docker image to registry
	docker push $(IMAGE_FULL)

.PHONY: docker-build-jupyter
docker-build-jupyter:  #-- Build JupyterLab Docker image
	docker build --build-arg GIT_TAG=$(GIT_TAG) -f .docker/jupyterlab.dockerfile --platform linux/x86_64 -t $(IMAGE):jupyter .

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

PYTEST_WORKERS ?= $(shell python3 -c "import os; print(min(64, os.cpu_count() or 64))")

.PHONY: pytest
pytest:  #-- Run Python tests with pytest in parallel with immediate failure reporting
	$(info $(M) Running Python tests in parallel with immediate failure reporting (workers=$(PYTEST_WORKERS))...)
	uv run --active --no-sync pytest --new-first --failed-first --tb=line -n $(PYTEST_WORKERS) --dist=loadgroup --maxfail=50 --durations=0 --durations-min=10.0

.PHONY: test-performance
test-performance:  #-- Run performance tests with codspeed benchmarking
	uv run --active --no-sync pytest tests/performance_tests --benchmark-disable-gc --codspeed

#== v2 (python/)
# Unset VIRTUAL_ENV so uv targets the python/.venv, not the parent v1 venv.

.PHONY: sync-v2
sync-v2:  #-- Sync v2 Python dependencies (without building the package)
	$(info $(M) Syncing v2 Python dependencies...)
	$Q cd python && VIRTUAL_ENV= uv sync --all-groups --no-install-package nautilus-trader --inexact

.PHONY: build-debug-v2
build-debug-v2: sync-v2  #-- Build the v2 Python package in debug mode (fast incremental builds)
	$(info $(M) Building v2 extension in debug mode...)
	$Q cd python && VIRTUAL_ENV= CARGO_TARGET_DIR=../target-v2 uv run --no-sync maturin develop

.PHONY: py-stubs-v2
py-stubs-v2:  #-- Regenerate v2 Python type stubs from Rust bindings
	$(info $(M) Generating v2 Python type stubs...)
	$Q CARGO_TARGET_DIR=target-v2 python python/generate_stubs.py

.PHONY: update-v2
update-v2: cargo-update  #-- Update v2 dependencies (cargo and uv)
	$(info $(M) Updating v2 uv lockfile...)
	$Q cd python && VIRTUAL_ENV= uv lock --upgrade

.PHONY: pytest-v2
pytest-v2: build-debug-v2  #-- Run v2 Python tests
	$(info $(M) Running v2 Python tests...)
	$Q cd python && VIRTUAL_ENV= uv run --no-sync pytest tests/ -v --ignore=tests/unit/test_live_node.py
	$Q cd python && VIRTUAL_ENV= uv run --no-sync pytest tests/unit/test_live_node.py -v

.PHONY: pre-flight-v2
pre-flight-v2: export CARGO_TARGET_DIR=target-v2
pre-flight-v2:  #-- Run comprehensive v2 pre-flight checks (format, check-code, cargo-test, build, pytest)
	$(info $(M) Running v2 pre-flight checks...)
	@if ! git diff --quiet; then \
		printf "$(RED)ERROR: You have unstaged changes$(RESET)\n"; \
		printf "$(YELLOW)Stage your changes first:$(RESET) git add .\n"; \
		exit 1; \
	fi
	@$(MAKE) --no-print-directory install-deps
	@$(MAKE) --no-print-directory format
	@$(MAKE) --no-print-directory check-code EXTRA_FEATURES="capnp,hypersync"
	@$(MAKE) --no-print-directory cargo-test-extras
	@$(MAKE) --no-print-directory build-debug-v2
	@$(MAKE) --no-print-directory pytest-v2
	@printf "$(GREEN)All v2 pre-flight checks passed$(RESET)\n"

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
	@printf "$(GRAY)      Use $(CYAN)make <target> VERBOSE=false$(GRAY) to disable verbose output for build-debug and cargo-test$(RESET)\n\n"

	@printf "⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣠⣴⣶⡟⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀\n"
	@printf "⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣰⣾⣿⣿⣿⠀⢸⣿⣿⣿⣿⣶⣶⣤⣀⠀⠀⠀⠀⠀\n"
	@printf "⠀⠀⠀⠀⠀⠀⢀⣴⡇⢀⣾⣿⣿⣿⣿⣿⠀⣾⣿⣿⣿⣿⣿⣿⣿⠿⠓⠀⠀⠀⠀\n"
	@printf "⠀⠀⠀⠀⠀⣰⣿⣿⡀⢸⣿⣿⣿⣿⣿⣿⠀⣿⣿⣿⣿⣿⣿⠟⠁⣠⣄⠀⠀⠀⠀\n"
	@printf "⠀⠀⠀⠀⢠⣿⣿⣿⣇⠀⢿⣿⣿⣿⣿⣿⠀⢻⣿⣿⣿⡿⢃⣠⣾⣿⣿⣧⡀⠀⠀\n"
	@printf "⠀⠀⠀⠠⣾⣿⣿⣿⣿⣿⣧⠈⠋⢀⣴⣧⠀⣿⡏⢠⡀⢸⣿⣿⣿⣿⣿⣿⣿⡇⠀\n"
	@printf "⠀⠀⠀⣀⠙⢿⣿⣿⣿⣿⣿⠇⢠⣿⣿⣿⡄⠹⠃⠼⠃⠈⠉⠛⠛⠛⠛⠛⠻⠇⠀\n"
	@printf "⠀⠀⢸⡟⢠⣤⠉⠛⠿⢿⣿⠀⢸⣿⡿⠋⣠⣤⣄⠀⣾⣿⣿⣶⣶⣶⣦⡄⠀⠀⠀\n"
	@printf "⠀⠀⠸⠀⣾⠏⣸⣷⠂⣠⣤⠀⠘⢁⣴⣾⣿⣿⣿⡆⠘⣿⣿⣿⣿⣿⣿⠀⠀⠀⠀\n"
	@printf "⠀⠀⠀⠀⠛⠀⣿⡟⠀⢻⣿⡄⠸⣿⣿⣿⣿⣿⣿⣿⡀⠘⣿⣿⣿⣿⠟⠀⠀⠀⠀\n"
	@printf "⠀⠀⠀⠀⠀⠀⣿⠇⠀⠀⢻⡿⠀⠈⠻⣿⣿⣿⣿⣿⡇⠀⢹⣿⠿⠋⠀⠀⠀⠀⠀\n"
	@printf "⠀⠀⠀⠀⠀⠀⠋⠀⠀⠀⡘⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠁⠀⠀⠀⠀⠀⠀⠀\n"

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
