PROJECT?=nautechsystems/nautilus_trader
REGISTRY?=ghcr.io/
IMAGE?=$(REGISTRY)$(PROJECT)
GIT_TAG:=$(shell git rev-parse --abbrev-ref HEAD)
IMAGE_FULL?=$(IMAGE):$(GIT_TAG)

.PHONY: install
install:
	BUILD_MODE=release uv sync --active --all-groups --all-extras --verbose

.PHONY: install-debug
install-debug:
	BUILD_MODE=debug uv sync --active --all-groups --all-extras --verbose

.PHONY: install-just-deps
install-just-deps:
	uv sync --active --all-groups --all-extras --no-install-package nautilus_trader

.PHONY: build
build:
	BUILD_MODE=release uv run --active --no-sync build.py

.PHONY: build-debug
build-debug:
	BUILD_MODE=debug uv run --active --no-sync build.py

.PHONY: build-wheel
build-wheel:
	BUILD_MODE=release uv build --wheel

.PHONY: build-wheel-debug
build-wheel-debug:
	BUILD_MODE=debug uv build --wheel

.PHONY: build-dry-run
build-dry-run:
	DRY_RUN=true uv run --active --no-sync build.py

.PHONY: clean
clean: clean-build-artifacts clean-caches clean-builds

.PHONY: clean-builds
clean-builds:
		rm -rf dist target 2>/dev/null || true

.PHONY: clean-build-artifacts
clean-build-artifacts:
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
clean-caches:
	rm -rf .pytest_cache .mypy_cache .ruff_cache 2>/dev/null || true
	-uv cache prune
	-cargo clean

.PHONY: distclean
distclean: clean
	@[ "$$FORCE" = 1 ] || { echo "Pass FORCE=1 to really nuke"; exit 1; }
	@echo "⚠️  nuking working tree (git clean -fxd)…"
	git clean -fxd -e tests/test_data/large/ -e .venv

.PHONY: format
format:
	cargo +nightly fmt

.PHONY: pre-commit
pre-commit:
	uv run --active --no-sync pre-commit run --all-files

.PHONY: ruff
ruff:
	uv run --active --no-sync ruff check . --fix

.PHONY: outdated
outdated:
	cargo outdated

.PHONY: update cargo-update
update: cargo-update
	uv self update
	uv lock --upgrade

.PHONY: docs
docs: docs-python docs-rust

.PHONY: docs-python
docs-python:
	BUILD_MODE=debug uv run --active sphinx-build -M markdown ./docs/api_reference ./api_reference

.PHONY: docs-rust
docs-rust:
	RUSTDOCFLAGS="--enable-index-page -Zunstable-options" cargo +nightly doc --all-features --no-deps --workspace

.PHONY: docsrs-check
docsrs-check:
	RUSTDOCFLAGS="--cfg docsrs -D warnings" cargo hack --workspace doc --no-deps --all-features

.PHONY: clippy
clippy:
	cargo clippy --fix --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery -W clippy::unwrap_used -W clippy::expect_used

.PHONY: clippy-nightly
clippy-nightly:
	cargo +nightly clippy --fix --all-targets --all-features --allow-dirty --allow-staged -- -D warnings -W clippy::pedantic -W clippy::nursery -W clippy::unwrap_used -W clippy::expect_used

.PHONY: cargo-build
cargo-build:
	cargo build --release --all-features

.PHONY: cargo-update
cargo-update:
	cargo update \
	&& cargo install cargo-nextest \
	&& cargo install cargo-llvm-cov

.PHONY:
cargo-check:
	cargo check --workspace --all-features

.PHONY: check-nextest
check-nextest:
	@if ! cargo nextest --version >/dev/null 2>&1; then \
		echo "cargo-nextest is not installed. You can install it using 'cargo install cargo-nextest'"; \
		exit 1; \
	fi

.PHONY: cargo-test
cargo-test: RUST_BACKTRACE=1
cargo-test: HIGH_PRECISION=true
cargo-test: check-nextest
cargo-test:
	cargo nextest run --workspace --features "ffi,python,high-precision,defi" --no-fail-fast --cargo-profile nextest

.PHONY: cargo-test-lib
cargo-test: RUST_BACKTRACE=1
cargo-test: HIGH_PRECISION=true
cargo-test-lib: check-nextest
cargo-test-lib:
	cargo nextest run --lib --workspace --no-default-features --features "ffi,python,high-precision,defi,stubs" --no-fail-fast --cargo-profile nextest

.PHONY: cargo-test-standard-precision
cargo-test-standard-precision: RUST_BACKTRACE=1
cargo-test-standard-precision: HIGH_PRECISION=false
cargo-test-standard-precision: check-nextest
cargo-test-standard-precision:
	cargo nextest run --workspace --features "ffi,python" --no-fail-fast --cargo-profile nextest

.PHONY: cargo-test-debug
cargo-test-debug: RUST_BACKTRACE=1
cargo-test-debug: HIGH_PRECISION=true
cargo-test-debug: check-nextest
cargo-test-debug:
	cargo nextest run --workspace --features "ffi,python,high-precision,defi" --no-fail-fast

.PHONY: cargo-test-standard-precision-debug
cargo-test-standard-precision-debug: RUST_BACKTRACE=1
cargo-test-standard-precision-debug: HIGH_PRECISION=false
cargo-test-standard-precision-debug: check-nextest
cargo-test-standard-precision-debug:
	cargo nextest run --workspace --features "ffi,python"

.PHONY: cargo-test-coverage
cargo-test-coverage: check-nextest
cargo-test-coverage:
	@if ! cargo llvm-cov --version >/dev/null 2>&1; then \
		echo "cargo-llvm-cov is not installed. You can install it using 'cargo install cargo-llvm-cov'"; \
		exit 1; \
	fi
	cargo llvm-cov nextest run --workspace

# -----------------------------------------------------------------------------
# Library tests for a single crate
# -----------------------------------------------------------------------------
# Invoke as:
#   make cargo-test-crate-<crate_name>
# Example:
#   make cargo-test-crate-nautilus_macro
#
# This reuses the same flags as `cargo-test-lib` but targets only the specified
# crate by replacing `--workspace` with `-p <crate>`.
# -----------------------------------------------------------------------------

.PHONY: cargo-test-crate-%
cargo-test-crate-%: RUST_BACKTRACE=1
cargo-test-crate-%: HIGH_PRECISION=true
cargo-test-crate-%: check-nextest
cargo-test-crate-%:
	cargo nextest run --lib --no-default-features --all-features --no-fail-fast --cargo-profile nextest -p $*

.PHONY: cargo-bench
cargo-bench:
	cargo bench

.PHONY: cargo-doc
cargo-doc:
	cargo doc

.PHONY: docker-build
docker-build: clean
	docker pull $(IMAGE_FULL) || docker pull $(IMAGE):nightly || true
	docker build -f .docker/nautilus_trader.dockerfile --platform linux/x86_64 -t $(IMAGE_FULL) .

.PHONY: docker-build-force
docker-build-force:
	docker build --no-cache -f .docker/nautilus_trader.dockerfile -t $(IMAGE_FULL) .

.PHONY: docker-push
docker-push:
	docker push $(IMAGE_FULL)

.PHONY: docker-build-jupyter
docker-build-jupyter:
	docker build --build-arg GIT_TAG=$(GIT_TAG) -f .docker/jupyterlab.dockerfile --platform linux/x86_64 -t $(IMAGE):jupyter .

.PHONY: docker-push-jupyter
docker-push-jupyter:
	docker push $(IMAGE):jupyter

.PHONY: start-services
start-services:
	docker-compose -f .docker/docker-compose.yml up -d

.PHONY: stop-services
stop-services:
	docker-compose -f .docker/docker-compose.yml down

.PHONY: pytest
pytest:
	uv run --active --no-sync pytest --new-first --failed-first

.PHONY: test-performance
test-performance:
	uv run --active --no-sync pytest tests/performance_tests --benchmark-disable-gc --codspeed

.PHONY: install-cli
install-cli:
	cargo install --path crates/cli --bin nautilus --force
