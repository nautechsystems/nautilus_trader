#!/bin/bash

export PROFILE_MODE=true
poetry install --with test --all-extras
poetry run pytest --ignore=tests/performance_tests --cov-report=term --cov-report=xml --cov=nautilus_trader --new-first --failed-first
