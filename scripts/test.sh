#!/bin/bash

poetry install --with test --all-extras
poetry run pytest --ignore=tests/performance_tests -k "not no_ci" --new-first --failed-first
