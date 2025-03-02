#!/bin/bash

uv sync --all-groups --all-extras
uv run --no-sync pytest --ignore=tests/performance_tests --new-first --failed-first
