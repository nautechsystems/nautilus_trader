#!/bin/bash

poetry install --with test --all-extras
poetry run pytest tests/performance_tests
