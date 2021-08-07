EXTRAS?=	"betfair ccxt distributed docs ib"
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
	rm -rf dist
	rm -rf docs/build
	find . -name dask-worker-space -type d -exec rm -rf {} +
	find . -name .benchmarks -type d -exec rm -rf {} +
	find . -name '*.dll' -exec rm {} +
	find . -name '*.prof' -exec rm {} +
	find . -name '*.pyc' -exec rm {} +
	find . -name '*.pyo' -exec rm {} +
	find . -name '*.so' -exec rm {} +
	find . -name '*.c' -not -path ".nautilus_trader/msgbus/*" -exec rm {} +
	find . -name '*.o' -exec rm {} +
	rm -f coverage.xml
	rm -f dump.rdb

clean-build: clean build

docs:
	poetry run sphinx-build docs/source docs/build

pre-commit:
	pre-commit run --all-files
