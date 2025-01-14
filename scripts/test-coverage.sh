#!/bin/bash
set -eo pipefail

export PROFILE_MODE=true
poetry run pip install --force-reinstall "Cython==3.0.11"  # Temporarily to ensure v3.0.11 otherwise coverage fails
poetry install --with test --all-extras
poetry run pytest \
    --cov-report=term \
    --cov-report=xml \
    --cov=nautilus_trader
