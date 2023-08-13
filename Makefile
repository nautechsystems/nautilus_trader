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
	BUILD_MODE=debug poetry install --with dev,test --all-extras

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
	git clean -fxd

.PHONY: format
format:
	(cd nautilus_core && cargo +nightly fmt)

.PHONY: pre-commit
pre-commit: format
	pre-commit run --all-files

.PHONY: pre-flight
pre-flight: format pre-commit cargo-test build-debug pytest

.PHONY: ruff
ruff:
	ruff check . --fix

.PHONY: update
update:
	(cd nautilus_core && cargo update)
	poetry update
	poetry install --with dev,test --all-extras --no-root

.PHONY: docs
docs: docs-python docs-rust

.PHONY: docs-python
docs-python: install-just-deps-all
	poetry run sphinx-build docs docs/build/html -b html

.PHONY: docs-rust
docs-rust:
	(cd nautilus_core && RUSTDOCFLAGS="--enable-index-page -Zunstable-options" cargo +nightly doc --no-deps)

.PHONY: clippy
clippy:
	(cd nautilus_core && cargo clippy --fix --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery -W clippy::unwrap_used -W clippy::expect_used)

.PHONY: cargo-build
cargo-build:
	(cd nautilus_core && cargo build --release --all-features)

.PHONY: cargo-update
cargo-update:
	(cd nautilus_core && cargo update)

.PHONY: cargo-test
cargo-test:
	RUST_BACKTRACE=1 && (cd nautilus_core && cargo test)

.PHONY: cargo-test-nightly
cargo-test-nightly:
	RUST_BACKTRACE=1 && (cd nautilus_core && cargo +nightly test)

.PHONY: cargo-bench
cargo-bench:
	(cd nautilus_core && cargo bench)

.PHONY: cargo-doc
cargo-doc:
	(cd nautilus_core && cargo doc)

.PHONY: docker-build
docker-build: clean
	docker pull ${IMAGE_FULL} || docker pull ${IMAGE}:develop ||  true
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

.PHONY: pytest
pytest:
	bash scripts/test.sh

.PHONY: pytest-coverage
pytest-coverage:
	bash scripts/test-coverage.sh

.PHONY: test-examples
test-examples:
	bash scripts/test-examples.sh
