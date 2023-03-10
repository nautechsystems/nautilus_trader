#!/bin/bash

export PROFILE_MODE=true
poetry install --with test --all-extras
poetry run pytest --ignore=tests/performance_tests -k "not no_ci" --cov-report=term --cov-report=xml --cov=nautilus_trader --new-first --failed-first
