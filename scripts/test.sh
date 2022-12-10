#!/bin/bash

poetry run pytest --ignore=tests/performance_tests --new-first --failed-first 
