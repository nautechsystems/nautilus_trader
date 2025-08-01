#!/bin/bash

pass_count=1

for stub in $(find stubs -name "*.pyi"); do
    rel_path="${stub#stubs/}"
    target="nautilus_trader/${rel_path%.pyi}.pyx"
    
    if [[ ! -f "$target" ]]; then
        echo "ERROR: Missing implementation for stub: $stub" >&2
        echo "Expected: $target" >&2
        exit 1
    fi

    echo ""
    echo "# $pass_count"
    python3 stubs/validate_pyx_stubs.py "$target" "$stub"
    if [[ $? -eq 1 ]]; then
        echo "ERROR: Validation failed for stub: $stub" >&2
        exit 1
    fi

    pass_count=$((pass_count+1))
done

exit 0
