#!/bin/bash

export PROFILE_MODE=true
poetry run pytest --ignore=tests/performance_tests --cov-report=term --cov-report=xml --cov=nautilus_trader --new-first --failed-first
