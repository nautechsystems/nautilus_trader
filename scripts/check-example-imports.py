#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Check example imports against the current NautilusTrader Python package.
"""

from __future__ import annotations

import ast
import json
import re
from collections.abc import Iterator
from contextlib import suppress
from functools import cache
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
EXAMPLES_ROOT = REPO_ROOT / "examples"
DOCS_ROOT = REPO_ROOT / "docs"
PACKAGE_ROOT = REPO_ROOT / "python"
PYTHON_FENCE = re.compile(r"^```python(?:[ \t].*)?\n(.*?)^```[ \t]*$", re.MULTILINE | re.DOTALL)


@cache
def _module_sources(module: str) -> tuple[Path, ...]:
    relative = Path(*module.split("."))
    candidates = (
        PACKAGE_ROOT / relative.with_suffix(".pyi"),
        PACKAGE_ROOT / relative.with_suffix(".py"),
        PACKAGE_ROOT / relative / "__init__.pyi",
        PACKAGE_ROOT / relative / "__init__.py",
    )
    return tuple(path for path in candidates if path.exists())


@cache
def _module_exports(module: str) -> frozenset[str]:
    names: set[str] = set()

    for path in _module_sources(module):
        tree = ast.parse(path.read_bytes(), filename=str(path))
        for node in tree.body:
            names.update(_node_exports(node))

    return frozenset(names)


def _node_exports(node: ast.stmt) -> set[str]:
    if isinstance(node, (ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef)):
        return {node.name}
    if isinstance(node, (ast.Import, ast.ImportFrom)):
        return {alias.asname or alias.name.rsplit(".", 1)[-1] for alias in node.names}
    if isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name):
        return {node.target.id}
    if not isinstance(node, ast.Assign):
        return set()

    names = {target.id for target in node.targets if isinstance(target, ast.Name)}
    if "__all__" not in names:
        return names

    with suppress(TypeError, ValueError):
        exports = ast.literal_eval(node.value)
        if isinstance(exports, (list, tuple, set)):
            names.update(name for name in exports if isinstance(name, str))

    return names


def _notebook_source(path: Path) -> Iterator[tuple[str, str]]:
    notebook = json.loads(path.read_text(encoding="utf-8"))

    for index, cell in enumerate(notebook.get("cells", [])):
        if cell.get("cell_type") != "code":
            continue

        lines = []
        for line in cell.get("source", []):
            stripped = line.lstrip()
            lines.append("\n" if stripped.startswith(("%", "!")) else line)

        yield f"{path.relative_to(REPO_ROOT)}:cell-{index}", "".join(lines)


def _example_sources() -> Iterator[tuple[str, str | bytes]]:
    for path in sorted(EXAMPLES_ROOT.rglob("*.py")):
        yield str(path.relative_to(REPO_ROOT)), path.read_bytes()

    for path in sorted(EXAMPLES_ROOT.rglob("*.ipynb")):
        yield from _notebook_source(path)

    for directory in ("getting_started", "how_to", "tutorials"):
        for path in sorted((DOCS_ROOT / directory).rglob("*.py")):
            yield str(path.relative_to(REPO_ROOT)), path.read_bytes()

    for path in sorted(DOCS_ROOT.rglob("*.md")):
        source = path.read_text(encoding="utf-8")
        for index, match in enumerate(PYTHON_FENCE.finditer(source), start=1):
            yield f"{path.relative_to(REPO_ROOT)}:python-block-{index}", match.group(1)

    for path in sorted(DOCS_ROOT.rglob("*.mdx")):
        source = path.read_text(encoding="utf-8")
        for index, match in enumerate(PYTHON_FENCE.finditer(source), start=1):
            yield f"{path.relative_to(REPO_ROOT)}:python-block-{index}", match.group(1)


def _check_imports(
    label: str,
    source: str | bytes,
    report_syntax: bool = True,
) -> list[str]:
    issues = []

    try:
        tree = ast.parse(source, filename=label)
    except SyntaxError as e:
        if report_syntax:
            return [f"{label}:{e.lineno}: invalid Python syntax: {e.msg}"]

        source_text = source.decode() if isinstance(source, bytes) else source
        import_lines = [
            line.strip()
            for line in source_text.splitlines()
            if line.strip().startswith(("from nautilus_trader", "import nautilus_trader"))
        ]
        return [
            issue
            for line_number, line in enumerate(import_lines, start=1)
            for issue in _check_imports(f"{label}:import-{line_number}", line)
        ]

    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            issues.extend(
                f"{label}:{node.lineno}: missing module {alias.name}"
                for alias in node.names
                if alias.name.startswith("nautilus_trader") and not _module_sources(alias.name)
            )
        elif (
            isinstance(node, ast.ImportFrom)
            and node.module
            and node.module.startswith("nautilus_trader")
        ):
            if not _module_sources(node.module):
                issues.append(f"{label}:{node.lineno}: missing module {node.module}")
                continue

            exports = _module_exports(node.module)
            for alias in node.names:
                submodule = f"{node.module}.{alias.name}"
                if (
                    alias.name != "*"
                    and alias.name not in exports
                    and not _module_sources(submodule)
                ):
                    issues.append(
                        f"{label}:{node.lineno}: {alias.name} is not exported by {node.module}",
                    )

    return issues


def main() -> int:
    issues = []
    checked = 0

    for label, source in _example_sources():
        checked += 1
        issues.extend(
            _check_imports(
                label,
                source,
                report_syntax=":python-block-" not in label,
            ),
        )

    if issues:
        print("Invalid NautilusTrader imports found in examples:")
        print()
        print("\n".join(issues))
        return 1

    print(f"All NautilusTrader imports resolve in {checked} example sources")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
