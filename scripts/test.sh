#!/bin/bash

poetry install --with test --extras "betfair docker ib redis"
poetry run pytest --ignore=tests/performance_tests --new-first --failed-first 
