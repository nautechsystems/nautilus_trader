#!/usr/bin/env bash

# Check for "codegen-backend" in TOML files
EXIT_CODE=0

for file in "$@"; do
  if grep -q "codegen-backend" "$file"; then
    echo "ERROR: $file contains the forbidden keyword 'codegen-backend'"
    EXIT_CODE=1
  fi
done

exit $EXIT_CODE
