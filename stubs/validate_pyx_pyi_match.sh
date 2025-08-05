#!/bin/bash

total_count=0
failed_count=0

for stub in $(find stubs -name "*.pyi"); do
    rel_path="${stub#stubs/}"
    target="nautilus_trader/${rel_path%.pyi}.pyx"
    
    if [[ ! -f "$target" ]]; then
        echo "ERROR: Missing implementation for stub: $stub" >&2
        echo "Expected: $target" >&2
        exit 1
    fi

    echo ""
    echo "# $total_count"
    python3 stubs/validate_pyx_stubs.py "$target" "$stub" -w
    if [[ $? -eq 1 ]]; then
        echo "ERROR: Validation failed for stub: $stub" >&2
        failed_count=$((failed_count+1))
    fi

    total_count=$((total_count+1))
done

echo "Total: $total_count Failed: $failed_count"
exit 0
