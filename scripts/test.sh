#!/bin/bash

uv sync
uv run build.py
uv run pytest --ignore=tests/performance_tests --new-first --failed-first
