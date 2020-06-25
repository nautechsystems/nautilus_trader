#!/bin/bash

pytest ../tests/unit_tests/
pytest ../tests/integration_tests/
pytest ../tests/performance_tests/
pytest ../tests/acceptance_tests/