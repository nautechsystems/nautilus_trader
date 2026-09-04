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
Check that every `nautilus_trader` import in the documentation still resolves.

Markdown code blocks and the panel-rendering scripts are not executed anywhere, so a
module move or a rename can leave them importing names that no longer exist. Snippets
are usually fragments rather than whole programs, so each import statement is parsed and
resolved on its own.

"""

from __future__ import annotations

import ast
import importlib
import re
import textwrap
from pathlib import Path

import pytest


DOCS_DIR = Path(__file__).resolve().parents[3] / "docs"

# Fences carry an info string in tabbed Rust/Python pairs, as in ```python tab="Python"
_PYTHON_BLOCK = re.compile(r"^```python[^\n]*\n(.*?)^```", re.DOTALL | re.MULTILINE)
_PYTHON_FENCE = re.compile(r"^```python", re.MULTILINE)
_IMPORT_START = re.compile(r"^\s*(?:from\s+nautilus_trader|import\s+nautilus_trader)\b")


def _import_statements(block: str) -> list[str]:
    statements = []
    lines = block.splitlines()
    index = 0

    while index < len(lines):
        if not _IMPORT_START.match(lines[index]):
            index += 1
            continue

        statement = lines[index]
        while statement.count("(") > statement.count(")"):
            index += 1
            statement += "\n" + lines[index]

        statements.append(statement)
        index += 1

    return statements


def _resolve(statement: str) -> str | None:
    """
    Return a failure reason for the import statement, or `None` when it resolves.
    """
    node = ast.parse(textwrap.dedent(statement)).body[0]

    if isinstance(node, ast.Import):
        for alias in node.names:
            try:
                importlib.import_module(alias.name)
            except ImportError as e:
                return f"{type(e).__name__}: {e}"
        return None

    assert isinstance(node, ast.ImportFrom)
    try:
        module = importlib.import_module(node.module or "")
    except ImportError as e:
        return f"{type(e).__name__}: {e}"

    missing = [alias.name for alias in node.names if not hasattr(module, alias.name)]
    if missing:
        return f"module has no attribute {', '.join(missing)}"

    return None


def _python_sources(path: Path) -> list[str]:
    """
    Return the Python sources to check for the documentation file at `path`.
    """
    if path.suffix == ".py":
        return [path.read_text(encoding="utf-8")]

    text = path.read_text(encoding="utf-8")
    blocks = _PYTHON_BLOCK.findall(text)

    # A fence the block pattern cannot read would skip its imports silently
    assert len(blocks) == len(_PYTHON_FENCE.findall(text)), f"unscanned python fence in {path.name}"

    return blocks


@pytest.mark.parametrize(
    "path",
    sorted([*DOCS_DIR.rglob("*.md"), *DOCS_DIR.rglob("*.py")]),
    ids=lambda p: f"{p.parent.name}/{p.name}",
)
def test_documentation_imports_resolve(path: Path) -> None:
    """
    Test every documented `nautilus_trader` import resolves against the current API.
    """
    failures = []

    for block in _python_sources(path):
        for statement in _import_statements(block):
            reason = _resolve(statement)
            if reason is not None:
                failures.append(f"{statement.strip()}  ->  {reason}")

    assert not failures, f"{path.relative_to(DOCS_DIR.parent)}:\n" + "\n".join(failures)
