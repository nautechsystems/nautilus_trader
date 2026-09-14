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
Test self-repository normalization and actionlint failure propagation.
"""

from __future__ import annotations

import os
import runpy
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from unittest.mock import patch


CHECKER_PATH = Path(__file__).with_name("check_actionlint.py")
CHECKER = runpy.run_path(str(CHECKER_PATH))


def _main() -> None:
    _test_normalize()
    _test_missing_executable()
    _test_workflows()
    sys.stdout.write("Actionlint compatibility tests passed\n")


def _test_normalize() -> None:
    source = """name: example
on: push
env:
  uses: $/unchanged-environment
jobs:
  reuse:
    uses: '$/.github/workflows/reusable.yml'
  build:
    runs-on: ubuntu-latest
    steps:
      - &local {uses: "$/.github/actions/local"} # preserved comment
      - *local
      - uses: actions/checkout@v7
      - uses: ./.github/actions/legacy
      - run: |
          uses: $/unchanged-script
"""
    expected = source.replace("$/.github/", "./.github/")
    _assert_equal(CHECKER["_normalize"](source), expected)
    composite = "runs: {using: composite, steps: [{uses: $/.github/actions/nested}]}\n"
    _assert_equal(CHECKER["_normalize"](composite), composite.replace("$/", "./"))
    _assert_equal(CHECKER["_normalize"](""), "")


def _test_missing_executable() -> None:
    with patch("shutil.which", return_value=None), patch("sys.stderr.write") as write:
        _assert_equal(CHECKER["_main"](), 1)
    write.assert_called_once_with("actionlint is required; run the actionlint pre-commit hook\n")


def _test_workflows() -> None:
    if shutil.which("actionlint") is None or shutil.which("shellcheck") is None:
        raise RuntimeError("actionlint and shellcheck are required for compatibility tests")

    with tempfile.TemporaryDirectory(prefix="nautilus-test-actionlint-") as directory:
        root = Path(directory)
        checker = root / ".pre-commit-hooks" / CHECKER_PATH.name
        checker.parent.mkdir()
        shutil.copyfile(CHECKER_PATH, checker)
        workflows = root / ".github" / "workflows"
        workflows.mkdir(parents=True)
        action = root / ".github" / "actions" / "local" / "action.yml"
        action.parent.mkdir(parents=True)
        action.write_text(
            "name: local\ndescription: Local action\n"
            "inputs:\n  required-value:\n    required: true\n    description: Required value\n"
            "runs:\n  using: composite\n  steps:\n    - shell: bash\n      run: echo ok\n",
        )
        reusable = workflows / "reusable.yml"
        reusable.write_text(
            "name: reusable\non:\n  workflow_call:\n    inputs:\n"
            "      required-value:\n        required: true\n        type: string\n"
            "jobs:\n  check:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo ok\n",
        )
        valid = (
            "name: caller\non: push\njobs:\n"
            "  reuse:\n    uses: $/.github/workflows/reusable.yml\n"
            "    with:\n      required-value: reusable-value\n"
            "  build:\n    runs-on: ubuntu-latest\n    steps:\n"
            "      - uses: $/.github/actions/local\n"
            "        with:\n          required-value: action-value\n"
        )
        cases = [
            (valid, ""),
            (valid.replace("          required-value: action-value\n", ""), "required-value"),
            (valid.replace("      required-value: reusable-value\n", ""), "required-value"),
            (
                valid.replace("required-value: action-value", "unknown-input: action-value"),
                "unknown-input",
            ),
            (
                valid.replace("required-value: reusable-value", "unknown-input: reusable-value"),
                "unknown-input",
            ),
            (valid + "      - run: echo $unquoted\n", "SC2086"),
        ]
        temporary = root / "temporary"
        temporary.mkdir()
        environment = dict(
            os.environ,
            TMPDIR=str(temporary),
            TEMP=str(temporary),
            TMP=str(temporary),
        )
        caller = workflows / "caller.yml"
        for content, diagnostic in cases:
            caller.write_text(content)
            before = {path: path.read_bytes() for path in root.rglob("*") if path.is_file()}
            result = subprocess.run(  # noqa: S603
                [sys.executable, "-B", str(checker)],
                cwd=root.parent,
                env=environment,
                capture_output=True,
                text=True,
                check=False,
            )
            output = result.stdout + result.stderr
            _assert_equal(result.returncode, 1 if diagnostic else 0, output)
            if diagnostic:
                _assert_equal(diagnostic in output, expected=True, context=output)
                _assert_equal(
                    ".github/workflows/caller.yml:" in output,
                    expected=True,
                    context=output,
                )
            after = {path: path.read_bytes() for path in root.rglob("*") if path.is_file()}
            _assert_equal(after, before)
            _assert_equal(list(temporary.iterdir()), [])


def _assert_equal(actual: object, expected: object, context: str = "") -> None:
    if actual != expected:
        raise AssertionError(f"Expected {expected!r}, received {actual!r}: {context}")


if __name__ == "__main__":
    _main()
