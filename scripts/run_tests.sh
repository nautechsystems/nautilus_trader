#!/bin/bash

python3 -m pytest ../tests/unit_tests/
python3 -m pytest ../tests/integration_tests/
python3 -m pytest ../tests/acceptance_tests/
