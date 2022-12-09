PROJECT?=nautechsystems/nautilus_trader
REGISTRY?=ghcr.io/
IMAGE?=${REGISTRY}${PROJECT}
GIT_TAG:=$(shell git rev-parse --abbrev-ref HEAD)
IMAGE_FULL?=${IMAGE}:${GIT_TAG}
EXTRAS?="betfair docker ib redis"
.PHONY: install build clean docs format pre-commit
.PHONY: cargo-update cargo-test cargo-test-arm64
.PHONY: update docker-build docker-build-force docker-push
.PHONY: docker-build-jupyter docker-push-jupyter

install:
	poetry install --extras ${EXTRAS}

build: nautilus_trader
	poetry run python build.py

clean:
	git clean -fxd

docs:
	poetry run sphinx-build docs docs/build/html -b html

format:
	(cd nautilus_core && cargo fmt)

pre-commit: format
	(cd nautilus_core && cargo fmt --all -- --check && cargo check -q && cargo clippy -- -D warnings)
	pre-commit run --all-files

cargo-update:
	(cd nautilus_core && cargo update)

cargo-test:
	(cd nautilus_core && cargo test)

cargo-test-arm64:
	(cd nautilus_core && cargo test --features extension-module)

update:
	(cd nautilus_core && cargo update)
	poetry update

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
