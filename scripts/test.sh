#!/bin/bash

uv sync --all-groups --all-extras
uv run build.py
uv run pytest --ignore=tests/performance_tests --new-first --failed-first
