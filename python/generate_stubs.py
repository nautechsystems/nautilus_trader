#!/usr/bin/env python3
"""
Custom build script for nautilus-trader v2 with automatic stub generation.

This script can be used as:
1. A standalone stub generator: python generate_stubs.py
2. A custom build script: python generate_stubs.py build
3. Integrated with uv/pip build systems via pyproject.toml

"""

import argparse
import subprocess
import sys
import tomllib
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


def load_pyproject():
    """
    Load the pyproject.toml located next to this script.
    """
    pyproject_path = Path(__file__).parent / "pyproject.toml"
    with pyproject_path.open("rb") as fp:
        return tomllib.load(fp)


def resolve_python_stub_root(pyproject: dict) -> Path:
    """
    Return the directory where pyo3-stub-gen writes .pyi files.
    """
    python_source = pyproject.get("tool", {}).get("maturin", {}).get("python-source", ".")

    dest_dir = (Path(__file__).parent / python_source).resolve()
    python_dir = Path(__file__).parent.resolve()
    try:
        dest_dir.relative_to(python_dir)
    except ValueError as exc:  # pragma: no cover - defensive
        raise RuntimeError(
            "python-source must stay within the python/ package directory",
        ) from exc

    dest_dir.mkdir(parents=True, exist_ok=True)
    return dest_dir


def generate_stubs():
    """
    Generate type stubs using pyo3-stub-gen.
    """
    print("Generating type stubs with pyo3-stub-gen...")

    crates_dir = Path(__file__).parent.parent / "crates" / "pyo3"
    pyproject = load_pyproject()
    dest_dir = resolve_python_stub_root(pyproject)
    module_name = pyproject.get("tool", {}).get("maturin", {}).get("module-name") or pyproject.get(
        "project",
        {},
    ).get("name", "nautilus_trader")
    module_root = module_name.split(".")[0]

    result = run_command(["cargo", "run", "--bin", "python-stub-gen"], cwd=crates_dir)

    print("Stubs generated successfully")
    if result.stdout:
        print(f"Output: {result.stdout}")

    stub_files = sorted(dest_dir.rglob("*.pyi"))

    if not stub_files:
        print("Stub generation completed but no .pyi files were produced")
        return False

    relocate_package_stubs(dest_dir)

    relative_root = dest_dir.relative_to(Path(__file__).parent)
    print(f"Type stubs written to {relative_root or Path('.')} ")

    preview_limit = 20
    filtered = [
        stub_file
        for stub_file in stub_files
        if stub_file.relative_to(dest_dir).parts
        and stub_file.relative_to(dest_dir).parts[0] == module_root
    ]

    targets = filtered if filtered else stub_files

    for stub_file in targets[:preview_limit]:
        print(f"Generated: {stub_file.relative_to(dest_dir)}")

    remaining = len(targets) - preview_limit
    if remaining > 0:
        print(f"...and {remaining} more stub files")

    return True


def relocate_package_stubs(dest_dir: Path) -> None:
    """
    Move top-level module stubs into package __init__.pyi files when needed.
    """
    core_stub = dest_dir / "nautilus_trader" / "core.pyi"
    if core_stub.exists():
        package_init = dest_dir / "nautilus_trader" / "core" / "__init__.pyi"
        package_init.parent.mkdir(parents=True, exist_ok=True)
        package_init.write_text(core_stub.read_text())
        core_stub.unlink()

    model_stub = dest_dir / "nautilus_trader" / "model.pyi"
    if model_stub.exists():
        package_init = dest_dir / "nautilus_trader" / "model" / "__init__.pyi"
        package_init.parent.mkdir(parents=True, exist_ok=True)
        package_init.write_text(model_stub.read_text())
        model_stub.unlink()


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
