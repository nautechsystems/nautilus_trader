#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Run actionlint with self-repository references resolved in a temporary copy.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import yaml
from yaml.nodes import MappingNode
from yaml.nodes import Node
from yaml.nodes import ScalarNode
from yaml.nodes import SequenceNode


def _main() -> int:
    executable = shutil.which("actionlint")
    if executable is None:
        sys.stderr.write("actionlint is required; run the actionlint pre-commit hook\n")
        return 1

    root = Path(__file__).resolve().parents[1]
    with tempfile.TemporaryDirectory(prefix="nautilus-actionlint-") as directory:
        snapshot = Path(directory)
        shutil.copytree(root / ".github", snapshot / ".github")

        # actionlint uses .git to discover local action metadata and reusable workflows
        (snapshot / ".git").mkdir()
        for folder in ("workflows", "actions"):
            for path in (snapshot / ".github" / folder).rglob("*"):
                if path.suffix in {".yaml", ".yml"}:
                    path.write_text(_normalize(path.read_text(encoding="utf-8")), encoding="utf-8")

        # Remove this compatibility copy when actionlint supports $/ references
        # https://github.com/rhysd/actionlint/issues/711
        # The executable comes from the pinned pre-commit environment
        return subprocess.run([executable], cwd=snapshot, check=False).returncode  # noqa: S603


# Keep YAML node selection and source offsets together so edits retain line and column positions
def _normalize(source: str) -> str:  # noqa: C901
    root = yaml.compose(source, Loader=yaml.SafeLoader)
    blocks = _values(root, "runs")
    for jobs in _values(root, "jobs"):
        if isinstance(jobs, MappingNode):
            blocks.extend(value for _, value in jobs.value)

    references = []
    for block in blocks:
        references.extend(_values(block, "uses"))
        for steps in _values(block, "steps"):
            if isinstance(steps, SequenceNode):
                for step in steps.value:
                    references.extend(_values(step, "uses"))

    offsets = set()
    for reference in references:
        if isinstance(reference, ScalarNode) and reference.value.startswith("$/"):
            offset = reference.start_mark.index + (reference.style in {"'", '"'})
            if source[offset : offset + 2] != "$/":
                raise ValueError("Self-repository references must use a literal $/ prefix")
            offsets.add(offset)

    for offset in sorted(offsets, reverse=True):
        source = source[:offset] + "." + source[offset + 1 :]
    return source


def _values(node: Node | None, key: str) -> list[Node]:
    if not isinstance(node, MappingNode):
        return []
    return [
        value for name, value in node.value if isinstance(name, ScalarNode) and name.value == key
    ]


if __name__ == "__main__":
    sys.exit(_main())
