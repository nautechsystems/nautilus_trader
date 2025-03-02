#!/bin/bash

uv sync --all-groups --all-extras
uv run --no-sync pytest tests/performance_tests --benchmark-disable-gc --codspeed
