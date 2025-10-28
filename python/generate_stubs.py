#!/usr/bin/env python3
"""
Custom build script for nautilus-trader v2 with automatic stub generation.

This script can be used as:
1. A standalone stub generator: python generate_stubs.py
2. A custom build script: python generate_stubs.py build
3. Integrated with uv/pip build systems via pyproject.toml

"""

import argparse
import re
import subprocess
import sys
import tomllib
from dataclasses import dataclass
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
    except ValueError as e:  # pragma: no cover - defensive
        raise RuntimeError(
            "python-source must stay within the python/ package directory",
        ) from e

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

    maturin_features = pyproject.get("tool", {}).get("maturin", {}).get("features", [])
    cargo_features = [f for f in maturin_features if f != "extension-module"]

    cmd = ["cargo", "run", "--bin", "python-stub-gen"]
    if cargo_features:
        cmd.extend(["--features", ",".join(cargo_features)])

    result = run_command(cmd, cwd=crates_dir)

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
    root = dest_dir / "nautilus_trader"
    if not root.exists():  # pragma: no cover - defensive
        return

    for stub_path in sorted(root.rglob("*.pyi")):
        if stub_path.name == "__init__.pyi" or stub_path.stem == "_libnautilus":
            continue

        package_dir = stub_path.with_suffix("")
        package_init = package_dir / "__init__.pyi"

        package_init.parent.mkdir(parents=True, exist_ok=True)
        package_init.write_text(stub_path.read_text())
        stub_path.unlink()

    relocate_class_stubs(root)
    apply_runtime_module_fixups()


@dataclass(frozen=True)
class StubFixup:
    """
    Configuration describing how to relocate and patch stub content for a module.
    """

    classes: tuple[str, ...] = ()
    imports: tuple[str, ...] = ()
    placeholders: tuple[str, ...] = ()


METHOD_RENAMES = {
    "py_new": "__init__",
}

MODULE_FIXUPS: dict[str, StubFixup] = {
    "adapters.blockchain": StubFixup(
        classes=(
            "BlockchainDataClientConfig",
            "BlockchainDataClientFactory",
            "DexPoolFilters",
        ),
        imports=(
            "import builtins",
            "import typing",
            "import nautilus_trader.infrastructure",
            "import nautilus_trader.model",
        ),
    ),
    "model": StubFixup(
        classes=(
            "DataType",
            "AmmType",
            "DexType",
            "Dex",
            "Blockchain",
            "Chain",
        ),
        imports=(
            "import builtins",
            "import typing",
            "from enum import Enum",
        ),
        placeholders=(
            "",
            '__all__ = ["AmmType", "Blockchain", "Chain", "DataType", "Dex", "DexType"]',
        ),
    ),
    "infrastructure": StubFixup(
        classes=("PostgresConnectOptions",),
        imports=("import typing",),
        placeholders=(
            "class PostgresConnectOptions: ...",
            "",
            '__all__ = ["PostgresConnectOptions"]',
        ),
    ),
}

MODULE_RUNTIME_FIXUPS: dict[Path, str] = {
    Path(
        "nautilus_trader/adapters/blockchain/__init__.py",
    ): "nautilus_trader.core.nautilus_pyo3.blockchain",
    Path("nautilus_trader/model/__init__.py"): "nautilus_trader.core.nautilus_pyo3.model",
}

RUNTIME_FIXUP_TEMPLATE = """
def _reassign_module_names() -> None:
    for _name, _obj in list(globals().items()):
        module = getattr(_obj, "__module__", "")
        if module.startswith("{prefix}"):
            try:
                _obj.__module__ = __name__
            except (AttributeError, TypeError):
                continue


_reassign_module_names()
del _reassign_module_names
"""


def relocate_class_stubs(root: Path) -> None:
    lib_stub = root / "_libnautilus.pyi"
    if not lib_stub.exists():
        return

    source = lib_stub.read_text()

    remaining = source

    for module_suffix, fixup in MODULE_FIXUPS.items():
        remaining, blocks = extract_class_blocks(remaining, fixup.classes)

        if not blocks and not fixup.placeholders:
            continue

        module_parts = module_suffix.split(".")
        module_path = root.joinpath(*module_parts)
        module_path.mkdir(parents=True, exist_ok=True)
        target_file = module_path / "__init__.pyi"

        header = [
            "# This file is automatically generated by pyo3_stub_gen",
            "# ruff: noqa: D401, E501, F401",
            "",
        ]

        body_parts: list[str] = []
        if blocks:
            body_parts.append("\n\n".join(blocks))
        if fixup.placeholders:
            body_parts.append("\n".join(fixup.placeholders))

        imports_section = list(fixup.imports)
        content = (
            "\n".join(header + imports_section + ["", "\n".join(body_parts), ""]).strip("\n") + "\n"
        )
        target_file.write_text(content)

    lib_stub.write_text(remaining.strip() + "\n")


def extract_class_blocks(source: str, class_names: tuple[str, ...]) -> tuple[str, list[str]]:
    remaining = source
    blocks: list[str] = []

    for class_name in class_names:
        pattern = re.compile(
            rf"^class {class_name}(?:\([^)]*\))?:[\s\S]*?(?=^(?:class |def |@|$))",
            re.MULTILINE,
        )
        match = pattern.search(remaining)
        if not match:
            continue

        block = match.group().rstrip()
        block = rename_methods(block)
        blocks.append(block)
        remaining = remaining[: match.start()] + remaining[match.end() :]

    return remaining, blocks


def rename_methods(block: str) -> str:
    for source_name, target_name in METHOD_RENAMES.items():
        block = re.sub(rf"def\s+{source_name}(\s*\()", rf"def {target_name}\1", block)
    block = re.sub(r"(def __init__\(.*?\)) -> [^:]+:", r"\1 -> None:", block)
    return block


def apply_runtime_module_fixups() -> None:
    """
    Runtime alias fixups temporarily disabled during cleanup.
    """
    return


def build_extension():
    """
    Build the extension using maturin.
    """
    print("Building extension with maturin...")

    run_command(["maturin", "develop", "--release"])

    print("Extension built successfully")
    return True


def main():
    parser = argparse.ArgumentParser(description="NautilusTrader v2 build script")
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
