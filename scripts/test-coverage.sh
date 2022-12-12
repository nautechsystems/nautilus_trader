#!/bin/bash

export PROFILE_MODE=true
poetry install --with test --extras "betfair docker ib redis"
poetry run pytest --ignore=tests/performance_tests --cov-report=term --cov-report=xml --cov=nautilus_trader --new-first --failed-first
