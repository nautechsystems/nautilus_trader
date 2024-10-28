#!/bin/bash

poetry run pytest tests/performance_tests --benchmark-disable-gc
