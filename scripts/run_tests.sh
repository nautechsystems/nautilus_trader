#!/bin/bash

python -m pytest ../tests/unit_tests/
python -m pytest ../tests/integration_tests/
python -m pytest ../tests/performance_tests/
python -m pytest ../tests/acceptance_tests/