#!/bin/bash

uv sync
uv run build.py
uv run pytest tests/performance_tests --benchmark-disable-gc --codspeed
