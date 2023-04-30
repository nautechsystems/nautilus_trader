#!/bin/bash

poetry install --with test --all-extras
poetry run pytest --ignore=tests/performance_tests --new-first --failed-first
