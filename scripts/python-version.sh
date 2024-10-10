#!/bin/bash

echo "$(python --version | cut -d' ' -f2 | tr -d '\n')"
