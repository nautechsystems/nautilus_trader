#!/bin/bash

uv sync --all-groups --all-extras
uv run pytest tests/performance_tests --benchmark-disable-gc --codspeed
