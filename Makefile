# Variables
# -----------------------------------------------------------------------------
PROJECT?=nautechsystems/nautilus_trader
REGISTRY?=ghcr.io/
IMAGE?=$(REGISTRY)$(PROJECT)
GIT_TAG:=$(shell git rev-parse --abbrev-ref HEAD)
IMAGE_FULL?=$(IMAGE):$(GIT_TAG)

V = 0  # 0 / 1 - verbose mode
Q = $(if $(filter 1,$V),,@) # Quiet mode, suppress command output
M = $(shell printf "$(BLUE)>$(RESET)") # Message prefix for commands

# Verbose options for specific targets (defaults to true, can be overridden)
VERBOSE ?= true

# > Colors
RED    := $(shell tput -Txterm setaf 1)
GREEN  := $(shell tput -Txterm setaf 2)
YELLOW := $(shell tput -Txterm setaf 3)
BLUE   := $(shell tput -Txterm setaf 4)
PURPLE := $(shell tput -Txterm setaf 5)
CYAN   := $(shell tput -Txterm setaf 6)
GRAY   := $(shell tput -Txterm setaf 7)
RESET  := $(shell tput -Txterm sgr0)

.DEFAULT_GOAL := help

#== Installation

.PHONY: install
install:  #-- Install in release mode with all dependencies and extras
	$(info $(M) Installing Nautilus Trader in release mode with all dependencies and extras...)
	$Q BUILD_MODE=release uv sync --active --all-groups --all-extras --verbose

.PHONY: install-debug
install-debug:  #-- Install in debug mode for development
	$(info $(M) Installing Nautilus Trader in debug mode for development...)
	$Q BUILD_MODE=debug uv sync --active --all-groups --all-extras --verbose

.PHONY: install-just-deps
install-just-deps:  #-- Install dependencies only without building the package
	$(info $(M) Installing dependencies only without building the package...)
	$Q uv sync --active --all-groups --all-extras --no-install-package nautilus_trader

#== Build

.PHONY: build
build:  #-- Build the package in release mode
	BUILD_MODE=release uv run --active --no-sync build.py

.PHONY: build-debug
build-debug:  #-- Build the package in debug mode (recommended for development)
ifeq ($(VERBOSE),true)
	$(info $(M) Building in debug mode with verbose output...)
	BUILD_MODE=debug uv run --active --no-sync build.py
else
	$(info $(M) Building in debug mode (errors will still be shown)...)
	BUILD_MODE=debug uv run --active --no-sync build.py 2>&1 | grep -E "(Error|error|ERROR|Failed|failed|FAILED|Warning|warning|WARNING|Build completed|Build time:|Traceback)" || true
endif

.PHONY: build-debug-pyo3
build-debug-pyo3:  #-- Build the package with PyO3 debug symbols (for debugging Rust code)
ifeq ($(VERBOSE),true)
	$(info $(M) Building in debug mode with PyO3 debug symbols...)
	BUILD_MODE=debug-pyo3 uv run --active --no-sync build.py
else
	$(info $(M) Building in debug mode with PyO3 debug symbols (errors will still be shown)...)
	BUILD_MODE=debug-pyo3 uv run --active --no-sync build.py 2>&1 | grep -E "(Error|error|ERROR|Failed|failed|FAILED|Warning|warning|WARNING|Build completed|Build time:|Traceback)" || true
endif

.PHONY: build-wheel
build-wheel:  #-- Build wheel distribution in release mode
	BUILD_MODE=release uv build --wheel

.PHONY: build-wheel-debug
build-wheel-debug:  #-- Build wheel distribution in debug mode
	BUILD_MODE=debug uv build --wheel

.PHONY: build-dry-run
build-dry-run:  #-- Show build commands without executing them
	DRY_RUN=true uv run --active --no-sync build.py

#== Clean

.PHONY: clean
clean: clean-build-artifacts clean-caches clean-builds  #-- Clean all build artifacts, caches, and builds

.PHONY: clean-builds
clean-builds:  #-- Clean distribution and target directories
	$Q rm -rf dist target 2>/dev/null || true

.PHONY: clean-build-artifacts
clean-build-artifacts:  #-- Clean compiled artifacts (.so, .dll, .pyc files)
	@echo "Cleaning build artifacts..."
	# Clean Rust build artifacts (keep final libraries)
	find target -name "*.rlib" -delete 2>/dev/null || true
	find target -name "*.rmeta" -delete 2>/dev/null || true
	rm -rf target/*/build target/*/deps 2>/dev/null || true
	# Clean Python build artifacts
	rm -rf build/ 2>/dev/null || true
	find . -type d -name "__pycache__" -not -path "./.venv*" -print0 | xargs -0 rm -rf
	find . -type f -a \( -name "*.pyc" -o -name "*.pyo" \) -not -path "./.venv*" -print0 | xargs -0 rm -f
	find . -type f -a \( -name "*.so" -o -name "*.dll" -o -name "*.dylib" \) -not -path "./.venv*" -print0 | xargs -0 rm -f
	# Clean test artifacts
	rm -rf .coverage .benchmarks 2>/dev/null || true

.PHONY: clean-caches
clean-caches:  #-- Clean pytest, mypy, ruff, uv, and cargo caches
	rm -rf .pytest_cache .mypy_cache .ruff_cache 2>/dev/null || true
	-uv cache prune
	-cargo clean

.PHONY: distclean
distclean: clean  #-- Nuclear clean - remove all untracked files (requires FORCE=1)
	@[ "$$FORCE" = 1 ] || { echo "Pass FORCE=1 to really nuke"; exit 1; }
	@echo "⚠️  nuking working tree (git clean -fxd)…"
	git clean -fxd -e tests/test_data/large/ -e .venv

#== Code Quality

.PHONY: format
format:  #-- Format Rust code using nightly formatter
	cargo +nightly fmt

.PHONY: pre-commit
pre-commit:  #-- Run all pre-commit hooks on all files
	uv run --active --no-sync pre-commit run --all-files

.PHONY: ruff
ruff:  #-- Run ruff linter with automatic fixes
	uv run --active --no-sync ruff check . --fix

.PHONY: clippy
clippy:  #-- Run Rust clippy linter with fixes
	cargo clippy --fix --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery -W clippy::unwrap_used -W clippy::expect_used

.PHONY: clippy-nightly
clippy-nightly:  #-- Run Rust clippy linter with nightly toolchain
	cargo +nightly clippy --fix --all-targets --all-features --allow-dirty --allow-staged -- -D warnings -W clippy::pedantic -W clippy::nursery -W clippy::unwrap_used -W clippy::expect_used

.PHONY: clippy-crate-%
clippy-crate-%:  #-- Run clippy for a specific Rust crate (usage: make clippy-crate-<crate_name>)
	cargo clippy --all-targets --all-features -p $* -- -D warnings

#== Dependencies

.PHONY: outdated
outdated:  #-- Check for outdated Rust dependencies
	cargo outdated

.PHONY: update cargo-update
update: cargo-update  #-- Update all dependencies (uv and cargo)
	uv self update
	uv lock --upgrade

#== Documentation

.PHONY: docs
docs: docs-python docs-rust  #-- Build all documentation (Python and Rust)

.PHONY: docs-python
docs-python:  #-- Build Python documentation with Sphinx
	BUILD_MODE=debug uv run --active sphinx-build -M markdown ./docs/api_reference ./api_reference

.PHONY: docs-rust
docs-rust:  #-- Build Rust documentation with cargo doc
	RUSTDOCFLAGS="--enable-index-page -Zunstable-options" cargo +nightly doc --all-features --no-deps --workspace

.PHONY: docsrs-check
docsrs-check: check-hack-installed #-- Check documentation builds for docs.rs compatibility
	RUSTDOCFLAGS="--cfg docsrs -D warnings" cargo hack --workspace doc --no-deps --all-features

#== Rust Development

.PHONY: cargo-build
cargo-build:  #-- Build Rust crates in release mode
	cargo build --release --all-features

.PHONY: cargo-update
cargo-update:  #-- Update Rust dependencies and install test tools
	cargo update \
	&& cargo install cargo-nextest \
	&& cargo install cargo-llvm-cov

.PHONY: cargo-check
cargo-check:  #-- Check Rust code without building
	cargo check --workspace --all-features

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

.PHONY: check-hack-installed
check-hack-installed:  #-- Verify cargo-hack is installed
	@if ! cargo hack --version >/dev/null 2>&1; then \
		echo "cargo-hack is not installed. You can install it using 'cargo install cargo-hack'"; \
		exit 1; \
	fi

.PHONY: check-features  #-- Verify crate feature combinations compile correctly
check-features: check-hack-installed
	cargo hack check --each-feature

#== Rust Testing

.PHONY: cargo-test
cargo-test: RUST_BACKTRACE=1
cargo-test: HIGH_PRECISION=true
cargo-test: check-nextest-installed
cargo-test:  #-- Run all Rust tests with ffi,python,high-precision,defi features
ifeq ($(VERBOSE),true)
	$(info $(M) Running Rust tests with verbose output...)
	cargo nextest run --workspace --features "ffi,python,high-precision,defi" --no-fail-fast --cargo-profile nextest --verbose
else
	$(info $(M) Running Rust tests (showing summary and failures only)...)
	cargo nextest run --workspace --features "ffi,python,high-precision,defi" --no-fail-fast --cargo-profile nextest --status-level fail --final-status-level flaky
endif

.PHONY: cargo-test-lib
cargo-test-lib: RUST_BACKTRACE=1
cargo-test-lib: HIGH_PRECISION=true
cargo-test-lib: check-nextest-installed
cargo-test-lib:  #-- Run Rust library tests only with high precision
	cargo nextest run --lib --workspace --no-default-features --features "ffi,python,high-precision,defi,stubs" --no-fail-fast --cargo-profile nextest

.PHONY: cargo-test-standard-precision
cargo-test-standard-precision: RUST_BACKTRACE=1
cargo-test-standard-precision: HIGH_PRECISION=false
cargo-test-standard-precision: check-nextest-installed
cargo-test-standard-precision:  #-- Run Rust tests with standard precision (64-bit)
	cargo nextest run --workspace --features "ffi,python" --no-fail-fast --cargo-profile nextest

.PHONY: cargo-test-debug
cargo-test-debug: RUST_BACKTRACE=1
cargo-test-debug: HIGH_PRECISION=true
cargo-test-debug: check-nextest-installed
cargo-test-debug:  #-- Run Rust tests in debug mode with high precision
	cargo nextest run --workspace --features "ffi,python,high-precision,defi" --no-fail-fast

.PHONY: cargo-test-standard-precision-debug
cargo-test-standard-precision-debug: RUST_BACKTRACE=1
cargo-test-standard-precision-debug: HIGH_PRECISION=false
cargo-test-standard-precision-debug: check-nextest-installed
cargo-test-standard-precision-debug:  #-- Run Rust tests in debug mode with standard precision
	cargo nextest run --workspace --features "ffi,python"

.PHONY: cargo-test-coverage
cargo-test-coverage: check-nextest-installed check-llvm-cov-installed
cargo-test-coverage:  #-- Run Rust tests with coverage reporting
	cargo llvm-cov nextest run --workspace

# -----------------------------------------------------------------------------
# Library tests for a single crate
# -----------------------------------------------------------------------------
# Invoke as:
#   make cargo-test-crate-<crate_name>
# Examples:
#   make cargo-test-crate-nautilus-model
#   make cargo-test-crate-nautilus-core FEATURES="python,ffi"
#
# This reuses the same flags as `cargo-test-lib` but targets only the specified
# crate by replacing `--workspace` with `-p <crate>`.
# To include specific features, use the FEATURES variable with comma-separated values.
# -----------------------------------------------------------------------------

.PHONY: cargo-test-crate-%
cargo-test-crate-%: RUST_BACKTRACE=1
cargo-test-crate-%: HIGH_PRECISION=true
cargo-test-crate-%: check-nextest-installed
cargo-test-crate-%:  #-- Run Rust tests for a specific crate (usage: make cargo-test-crate-<crate_name>)
	cargo nextest run --lib --no-fail-fast --cargo-profile nextest -p $* $(if $(FEATURES),--features "$(FEATURES)")

.PHONY: cargo-test-coverage-crate-%
cargo-test-coverage-crate-%: RUST_BACKTRACE=1
cargo-test-coverage-crate-%: HIGH_PRECISION=true
cargo-test-coverage-crate-%: check-nextest-installed check-llvm-cov-installed
cargo-test-coverage-crate-%:  #-- Run Rust tests with coverage reporting for a specific crate (usage: make cargo-test-coverage-crate-<crate_name>)
	cargo llvm-cov nextest --lib --no-fail-fast --cargo-profile nextest -p $* $(if $(FEATURES),--features "$(FEATURES)")

#------------------------------------------------------------------------------
# Benchmarks
#------------------------------------------------------------------------------

# List of crates whose criterion/iai benches run in the performance workflow
CI_BENCH_CRATES := nautilus-core nautilus-model nautilus-common

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
	@echo "${PURPLE}Waiting for PostgreSQL to be ready...${RESET}"
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
	cat schema/sql/*.sql | docker exec -i nautilus-database psql -U nautilus -d nautilus

#== Python Testing

.PHONY: pytest
pytest:  #-- Run Python tests with pytest
ifeq ($(VERBOSE),true)
	$(info $(M) Running Python tests with verbose output...)
	uv run --active --no-sync pytest --new-first --failed-first -v
else
	$(info $(M) Running Python tests (showing failures and summary only)...)
	uv run --active --no-sync pytest --new-first --failed-first --tb=short
endif

.PHONY: test-performance
test-performance:  #-- Run performance tests with codspeed benchmarking
	uv run --active --no-sync pytest tests/performance_tests --benchmark-disable-gc --codspeed

#== CLI Tools

.PHONY: install-cli
install-cli:  #-- Install Nautilus CLI tool from source
	cargo install --path crates/cli --bin nautilus --force

#== Internal

.PHONY: help
help:  #-- Show this help message and exit
	@printf "Nautilus Trader Makefile\n\n"
	@printf "$(GREEN)Usage:$(RESET) make $(CYAN)<target>$(RESET)\n\n"
	@printf "$(GRAY)Tips: Use $(CYAN)make <target> V=1$(GRAY) for verbose output$(RESET)\n"
	@printf "$(GRAY)      Use $(CYAN)make <target> VERBOSE=false$(GRAY) to disable verbose output for build-debug, cargo-test, and pytest$(RESET)\n\n"

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
	BEGIN { FS = ":.*#--"; target_maxlen = 0 } \
	/^[$$()% a-zA-Z_-]+:.*?#--/ { \
		if (length($$1) > target_maxlen) target_maxlen = length($$1); \
		targets[NR] = $$1; descriptions[NR] = $$2; \
	} \
	/^#==/ { \
		groups[NR] = substr($$0, 5); \
	} \
	END { \
		for (i = 1; i <= NR; i++) { \
			if (groups[i]) { \
				printf "\n$(GREEN)%s:$(RESET)\n", groups[i]; \
			} else if (targets[i]) { \
				printf "  $(CYAN)%-*s$(RESET) %s\n", target_maxlen, targets[i], descriptions[i]; \
			} \
		} \
	}' $(MAKEFILE_LIST)
