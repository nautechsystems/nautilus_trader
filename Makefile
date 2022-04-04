PROJECT?=nautechsystems/nautilus_trader
REGISTRY?=ghcr.io/
IMAGE?=${REGISTRY}${PROJECT}
GIT_TAG:=$(shell git rev-parse --abbrev-ref HEAD)
IMAGE_FULL?=${IMAGE}:${GIT_TAG}
EXTRAS?="hyperopt ib redis"
.PHONY: build clean docs

install:
	poetry install --extras ${EXTRAS}

build: nautilus_trader
	poetry run python build.py

clean:
	rm -rf .mypy_cache
	rm -rf .nox
	rm -rf .pytest_cache
	rm -rf build
	rm -rf cython_debug
	rm -rf dist
	rm -rf docs/build
	find . -name .benchmarks -type d -exec rm -rf {} +
	find . -name '*.dll' -exec rm {} +
	find . -name '*.prof' -exec rm {} +
	find . -name '*.pyc' -exec rm {} +
	find . -name '*.pyo' -exec rm {} +
	find . -name '*.so' -exec rm {} +
	find . -name '*.o' -exec rm {} +
	find . -name '*.c' -exec rm {} +
	rm -f coverage.xml
	rm -f dump.rdb

docs:
	poetry run sphinx-build docs docs/build/html -b html

pre-commit:
	pre-commit run --all-files

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
