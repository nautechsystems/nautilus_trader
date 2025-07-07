#!/usr/bin/env python3
"""
Create PyO3 configuration file with stable Python paths.

This script creates a .pyo3-config.txt file that prevents PyO3 from rebuilding
when switching between different build commands. The file is always regenerated
to ensure it uses the current Python environment.

Why this is necessary:
- UV creates temporary Python environments that change paths on each build
- PyO3 detects these path changes and triggers 5-30 minute rebuilds
- This script provides stable paths that don't change between builds

See scripts/README.md for detailed explanation.
"""

import sys
import sysconfig
from pathlib import Path
from textwrap import dedent


def detect_python_build_flags():
    """
    Detect Python build flags using multiple methods for reliability.

    Returns a list of build flag strings that should be included in PyO3 config.
    """
    flags = []

    # Method 1: Direct sys attribute checking

    # Py_DEBUG: Check for debug build
    if hasattr(sys, 'gettotalrefcount'):
        flags.append('Py_DEBUG')

    # Py_TRACE_REFS: Memory tracing (only in debug builds)
    if hasattr(sys, 'getobjects'):
        flags.append('Py_TRACE_REFS')

    # Py_REF_DEBUG: Reference debugging
    if hasattr(sys, 'gettotalrefcount') or hasattr(sys, '_debugmallocstats'):
        if 'Py_REF_DEBUG' not in flags:  # Avoid duplicates
            # Note: Py_REF_DEBUG is typically implied by Py_DEBUG
            # but we check separately for completeness
            flags.append('Py_REF_DEBUG')

    # Method 2: Check sysconfig for additional flags
    config_vars = sysconfig.get_config_vars()

    # Check for flags that might not have sys attributes
    if config_vars.get('Py_REF_DEBUG') and 'Py_REF_DEBUG' not in flags:
        flags.append('Py_REF_DEBUG')

    # WITH_THREAD: Threading support
    # For Python 3.7+, threading is always enabled by default
    flags.append('WITH_THREAD')

    # Method 3: Check for platform-specific debug indicators
    if hasattr(sys, 'abiflags'):
        # Unix systems have abiflags
        abiflags = sys.abiflags
        if 'd' in abiflags and 'Py_DEBUG' not in flags:
            flags.append('Py_DEBUG')

    # Additional checks for less common flags
    py_limited_api = config_vars.get('Py_LIMITED_API')
    if py_limited_api:
        flags.append('Py_LIMITED_API')

    # Remove duplicates while preserving order
    seen = set()
    unique_flags = []
    for flag in flags:
        if flag not in seen:
            seen.add(flag)
            unique_flags.append(flag)

    return unique_flags


def get_python_lib_info():
    """Get Python library information for PyO3 config."""
    lib_dir = sysconfig.get_config_var('LIBDIR') or ''
    lib_name = sysconfig.get_config_var('LDLIBRARY') or f'python{sys.version_info.major}.{sys.version_info.minor}'
    # Clean up library name (remove extensions and prefixes)
    lib_name = lib_name.replace('.so', '').replace('.a', '').replace('.dylib', '').replace('.dll', '')
    if lib_name.startswith('lib'):
        lib_name = lib_name[3:]

    return lib_dir, lib_name


def create_pyo3_config(config_path: Path):
    """Create PyO3 configuration file."""
    lib_dir, lib_name = get_python_lib_info()
    pointer_width = 64 if sys.maxsize > 2**32 else 32

    # Detect build flags
    build_flags = detect_python_build_flags()
    build_flags_str = ','.join(build_flags) if build_flags else ''

    executable = sys.executable

    config_content = dedent(f"""\
        implementation=CPython
        version={sys.version_info.major}.{sys.version_info.minor}
        shared=true
        abi3=false
        lib_name={lib_name}
        lib_dir={lib_dir}
        executable={executable}
        pointer_width={pointer_width}
        build_flags={build_flags_str}
        suppress_build_script_link_lines=false\
    """)

    def print_config_info():
        print(f"  Python version: {sys.version_info.major}.{sys.version_info.minor}")
        print(f"  Executable: {executable}")
        print(f"  Library: {lib_name} in {lib_dir}")
        print(f"  Architecture: {pointer_width}-bit")
        print(f"  Build flags: {build_flags_str if build_flags_str else '(none)'}")

    # Check if file exists and content is identical
    # If the timestamp changes, it retriggers a rebuild
    if config_path.exists():
        existing_content = config_path.read_text()
        if existing_content == config_content:
            print(f"Skipped {config_path} (content unchanged):")
            print_config_info()
            return

    config_path.write_text(config_content)
    print(f"Created {config_path} with:")
    print_config_info()


def main():
    """Main entry point."""
    config_path = Path(".pyo3-config.txt")

    # Check and potentially regenerate the config file
    create_pyo3_config(config_path)


if __name__ == "__main__":
    main()