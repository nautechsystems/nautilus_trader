PROJECT?=nautechsystems/nautilus_trader
REGISTRY?=ghcr.io/
IMAGE?=${REGISTRY}${PROJECT}
GIT_TAG:=$(shell git rev-parse --abbrev-ref HEAD)
IMAGE_FULL?=${IMAGE}:${GIT_TAG}

.PHONY: install
install:
	BUILD_MODE=release poetry install --with dev,test --all-extras

.PHONY: install-debug
install-debug:
	BUILD_MODE=debug poetry install --with dev,test --all-extras --sync

.PHONY: install-docs
install-docs:
	BUILD_MODE=debug poetry install --with docs --all-extras

.PHONY: install-just-deps
install-just-deps:
	poetry install --with dev,test --all-extras --no-root

.PHONY: install-just-deps-all
install-just-deps-all:
	poetry install --with dev,test,docs --all-extras --no-root

.PHONY: build
build:
	BUILD_MODE=release poetry run python build.py

.PHONY: build-debug
build-debug:
	BUILD_MODE=debug poetry run python build.py

.PHONY: build-wheel
build-wheel:
	BUILD_MODE=release poetry build --format wheel

.PHONY: build-wheel-debug
build-wheel-debug:
	BUILD_MODE=debug poetry build --format wheel

.PHONY: clean
clean:
	find . -type d -name "__pycache" -print0 | xargs -0 rm -rf
	find . -type f -a \( -name "*.so" -o -name "*.dll" \) -print0 | xargs -0 rm -f
	rm -rf \
		.benchmarks/ \
		.mypy_cache/ \
		.pytest_cache/ \
		.ruff_cache/ \
		build/ \
		target/

.PHONY: distclean
distclean: clean
	git clean -fxd -e tests/test_data/large/

.PHONY: format
format:
	cargo +nightly fmt

.PHONY: pre-commit
pre-commit:
	poetry run pre-commit run --all-files

.PHONY: ruff
ruff:
	ruff check . --fix

# Requires cargo-outdated v0.16.0+
.PHONY: outdated
outdated:
	cargo outdated && poetry show --outdated

.PHONY: update cargo-update
update: cargo-update
	poetry update
	poetry install --with dev,test --all-extras --no-root

.PHONY: docs
docs: docs-python docs-rust

.PHONY: docs-python
docs-python: install-docs
	poetry run sphinx-build -M markdown ./docs/api_reference ./api_reference

.PHONY: docs-rust
docs-rust:
	RUSTDOCFLAGS="--enable-index-page -Zunstable-options" cargo +nightly doc --all-features --no-deps --workspace

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
	cargo update && cargo install cargo-nextest && cargo install cargo-llvm-cov

.PHONY: cargo-test
cargo-test: RUST_BACKTRACE=1
cargo-test: HIGH_PRECISION=true
cargo-test:
	@if ! cargo nextest --version >/dev/null 2>&1; then \
		echo "cargo-nextest is not installed. You can install it using 'cargo install cargo-nextest'"; \
		exit 1; \
	fi
	RUST_BACKTRACE=$(RUST_BACKTRACE) HIGH_PRECISION=$(HIGH_PRECISION) cargo nextest run --workspace --features "python,ffi,high-precision"

.PHONY: cargo-test-standard-precision
cargo-test-standard-precision: RUST_BACKTRACE=1
cargo-test-standard-precision: HIGH_PRECISION=false
cargo-test-standard-precision:
	@if ! cargo nextest --version >/dev/null 2>&1; then \
    echo "cargo-nextest is not installed. You can install it using 'cargo install cargo-nextest'"; \
    exit 1; \
	fi
	RUST_BACKTRACE=$(RUST_BACKTRACE) HIGH_PRECISION=$(HIGH_PRECISION) cargo nextest run --workspace --features "python,ffi"

.PHONY: cargo-test-coverage
cargo-test-coverage:
	@if ! cargo nextest --version >/dev/null 2>&1; then \
		echo "cargo-nextest is not installed. You can install it using 'cargo install cargo-nextest'"; \
		exit 1; \
	fi
	@if ! cargo llvm-cov --version >/dev/null 2>&1; then \
		echo "cargo-llvm-cov is not installed. You can install it using 'cargo install cargo-llvm-cov'"; \
		exit 1; \
	fi
	cargo llvm-cov nextest run --workspace

.PHONY: cargo-bench
cargo-bench:
	cargo bench

.PHONY: cargo-doc
cargo-doc:
	cargo doc

.PHONY: docker-build
docker-build: clean
	docker pull ${IMAGE_FULL} || docker pull ${IMAGE}:nightly ||  true
	docker build -f .docker/nautilus_trader.dockerfile --platform linux/x86_64 -t ${IMAGE_FULL} .

.PHONY: docker-build-force
docker-build-force:
	docker build --no-cache -f .docker/nautilus_trader.dockerfile -t ${IMAGE_FULL} .

.PHONY: docker-push
docker-push:
	docker push ${IMAGE_FULL}

.PHONY: docker-build-jupyter
docker-build-jupyter:
	docker build --build-arg GIT_TAG=${GIT_TAG} -f .docker/jupyterlab.dockerfile --platform linux/x86_64 -t ${IMAGE}:jupyter .

.PHONY: docker-push-jupyter
docker-push-jupyter:
	docker push ${IMAGE}:jupyter

.PHONY: start-services
start-services:
	docker-compose -f .docker/docker-compose.yml up -d

.PHONY: stop-services
stop-services:
	docker-compose -f .docker/docker-compose.yml down

.PHONY: pytest
pytest:
	bash scripts/test.sh

.PHONY: pytest-coverage
pytest-coverage:
	bash scripts/test-coverage.sh

.PHONY: test-performance
test-performance:
	bash scripts/test-performance.sh

.PHONY: test-examples
test-examples:
	bash scripts/test-examples.sh

.PHONY: install-cli
install-cli:
	cargo install --path crates/cli --bin nautilus --force

.PHONY: install-talib
install-talib:
	bash scripts/install-talib.sh
