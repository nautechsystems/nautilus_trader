#!/usr/bin/env python3
"""
Custom build script for nautilus-trader v2 with automatic stub generation.

This script can be used as:
1. A standalone stub generator: python generate_stubs.py
2. A custom build script: python generate_stubs.py build
3. Integrated with uv/pip build systems via pyproject.toml

"""

import argparse
import shutil
import subprocess
import sys
from pathlib import Path


def run_command(cmd, cwd=None, check=True):
    """
    Run the command and return the result.
    """
    print(f"Running: {' '.join(cmd)}")
    if cwd:
        print(f"  in: {cwd}")

    result = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)

    if check and result.returncode != 0:
        print(f"Error: {result.stderr}")
        raise subprocess.CalledProcessError(result.returncode, cmd)

    return result


def generate_stubs():
    """
    Generate type stubs using pyo3-stub-gen.
    """
    print("Generating type stubs with pyo3-stub-gen...")

    # Path to the crates/pyo3 directory
    crates_dir = Path(__file__).parent.parent / "crates" / "pyo3"

    # Generate stubs using cargo
    result = run_command(["cargo", "run", "--bin", "python-stub-gen"], cwd=crates_dir)

    print("Stubs generated successfully")
    if result.stdout:
        print(f"Output: {result.stdout}")

    # Find the generated stub directory
    target_dir = crates_dir / "target" / "pyo3-stub-gen"
    stub_dirs = list(target_dir.glob("_libnautilus*"))

    if not stub_dirs:
        print("No stub directory found")
        return False

    stub_dir = stub_dirs[0]
    print(f"Found stubs in: {stub_dir}")

    # Copy stubs to the Python package
    dest_dir = Path(__file__).parent / "nautilus_trader"

    # Clean existing stubs first
    for existing_stub in dest_dir.rglob("*.pyi"):
        existing_stub.unlink()
        print(f"Removed existing stub: {existing_stub.relative_to(dest_dir)}")

    # Copy new stubs
    for stub_file in stub_dir.rglob("*.pyi"):
        # Determine relative path from stub_dir
        rel_path = stub_file.relative_to(stub_dir)
        dest_file = dest_dir / rel_path

        # Create destination directory if needed
        dest_file.parent.mkdir(parents=True, exist_ok=True)

        # Copy the stub file
        shutil.copy2(stub_file, dest_file)
        print(f"Copied: {rel_path}")

    print("Type stubs copied to nautilus_trader/")
    return True


def build_extension():
    """
    Build the extension using maturin.
    """
    print("Building extension with maturin...")

    run_command(["maturin", "develop", "--release"])

    print("Extension built successfully")
    return True


def main():
    parser = argparse.ArgumentParser(description="Nautilus Trader v2 build script")
    parser.add_argument(
        "action",
        nargs="?",
        default="stubs",
        choices=["stubs", "build", "all"],
        help="Action to perform: stubs (default), build, or all",
    )

    args = parser.parse_args()

    print(f"Starting nautilus-trader v2 {args.action}...")

    try:
        if args.action in ["stubs", "all"]:
            if not generate_stubs():
                return 1

        if args.action in ["build", "all"]:
            if not build_extension():
                return 1

        print(f"{'Build' if args.action != 'stubs' else 'Stub generation'} completed successfully")
        return 0

    except subprocess.CalledProcessError as e:
        print(f"Build failed: {e}")
        return 1
    except Exception as e:
        print(f"Unexpected error: {e}")
        return 1


if __name__ == "__main__":
    sys.exit(main())
