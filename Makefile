PROJECT?=nautechsystems/nautilus_trader
REGISTRY?=ghcr.io/
IMAGE?=${REGISTRY}${PROJECT}
GIT_TAG:=$(shell git rev-parse --abbrev-ref HEAD)
IMAGE_FULL?=${IMAGE}:${GIT_TAG}
.PHONY: install build clean docs format pre-commit
.PHONY: clippy cargo-build cargo-update cargo-test
.PHONY: update docker-build docker-build-force docker-push
.PHONY: docker-build-jupyter docker-push-jupyter
.PHONY: pytest pytest-coverage

install:
	BUILD_MODE=release poetry install --with dev,test --all-extras

install-debug:
	BUILD_MODE=debug poetry install --with dev,test --all-extras

install-just-deps:
	poetry install --with dev,test --all-extras --no-root --sync

install-just-deps-all:
	poetry install --with dev,test,docs --all-extras --no-root

build: nautilus_trader
	BUILD_MODE=release poetry run python build.py

build-debug: nautilus_trader
	BUILD_MODE=debug poetry run python build.py

clean:
	git clean -fxd

docs:
	poetry run sphinx-build docs docs/build/html -b html

format:
	(cd nautilus_core && cargo fmt)

pre-commit: format
	pre-commit run --all-files

ruff:
	ruff check . --fix

update:
	(cd nautilus_core && cargo update)
	poetry update

clippy:
	(cd nautilus_core && cargo clippy --fix --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery -W clippy::unwrap_used -W clippy::expect_used)

cargo-build:
	(cd nautilus_core && cargo build --release --all-features)

cargo-update:
	(cd nautilus_core && cargo update)

cargo-test:
	(cd nautilus_core && cargo test)

cargo-bench:
	(cd nautilus_core && cargo bench)

cargo-doc:
	(cd nautilus_core && cargo doc)

docker-build: clean
	docker pull ${IMAGE_FULL} || docker pull ${IMAGE}:develop ||  true
	docker build -f .docker/nautilus_trader.dockerfile --platform linux/x86_64 -t ${IMAGE_FULL} .

docker-build-force:
	docker build --no-cache -f .docker/nautilus_trader.dockerfile -t ${IMAGE_FULL} .

docker-push:
	docker push ${IMAGE_FULL}

docker-build-jupyter:
	docker build --build-arg GIT_TAG=${GIT_TAG} -f .docker/jupyterlab.dockerfile --platform linux/x86_64 -t ${IMAGE}:jupyter .

docker-push-jupyter:
	docker push ${IMAGE}:jupyter

pytest:
	bash scripts/test.sh

pytest-coverage:
	bash scripts/test-coverage.sh

test-examples:
	bash scripts/test-examples.sh
