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
Check the wire-literal spelling policy, optionally using the pinned spelling tools.

The default configuration probe uses Python regexes, not either spelling tool.
"""

from __future__ import annotations

import argparse
import configparser
import re
import shutil
import subprocess
import tempfile
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
ENCODED = '"tag=caf%C3%A9+trade%2F%25"'

# Construct intentional spelling errors without exempting them in repository source
FRAGMENT = bytes((99, 97, 102)).decode("ascii")
MISSPELLING = bytes((116, 101, 104)).decode("ascii")


def _policy_patterns() -> tuple[re.Pattern[str], re.Pattern[str]]:
    with ROOT.joinpath(".typos.toml").open("rb") as source:
        typos = tomllib.load(source)

    # Match the pinned codespell reader's treatment of percent characters
    codespell = configparser.ConfigParser(interpolation=None)
    codespell.read(ROOT / ".codespellrc", encoding="utf-8")
    codespell_pattern = codespell["codespell"]["ignore-regex"]
    candidates = typos["default"]["extend-ignore-re"]
    matching = [
        pattern
        for pattern in candidates
        if "".join(pattern.split()) == "".join(codespell_pattern.split())
    ]
    if len(matching) != 1:
        raise AssertionError("Expected one identical wire-literal pattern in both configurations")
    return re.compile(matching[0]), re.compile(codespell_pattern)


def _test_configuration() -> None:
    accepted = (
        ENCODED,
        ENCODED.replace("%C3%A9", "%c3%a9").replace("%2F", "%2f"),
        '"%74ag=value"',
        '"%74ag=%25"',
        '"%74ag="',
        '"tag=%25%2F"',
        '"a_b.c*-+=%20"',
    )
    rejected = (
        FRAGMENT,
        f'"{FRAGMENT}"',
        f'"tag={FRAGMENT}"',
        f'"tag={FRAGMENT}%"',
        f'"tag={FRAGMENT}%A"',
        f'"tag={FRAGMENT}%G3"',
        f'"tag={FRAGMENT}%C3%A9%"',
        f'"tag={FRAGMENT}%C3%A9 value"',
        f'"tag={FRAGMENT}%C3%A9/value"',
        f'"%G3tag={FRAGMENT}"',
        f'"%74ag%={FRAGMENT}"',
        '"=%20"',
        '"tag=%20&other=value"',
        '"tag=%20=other"',
        '"ordinary text%20"',
        '"https://example.com/?tag=%20"',
        "'tag=%20'",
        "tag=%20",
    )
    for pattern in _policy_patterns():
        for text in accepted:
            if pattern.fullmatch(text) is None:
                raise AssertionError(f"Wire literal was not matched: {text!r}")
        for text in rejected:
            if pattern.search(text) is not None:
                raise AssertionError(f"Ordinary or malformed text was ignored: {text!r}")
        for neighbor in (FRAGMENT, MISSPELLING):
            text = f"{neighbor} {ENCODED} {neighbor}"
            if pattern.sub(" ", text) != f"{neighbor}   {neighbor}":
                raise AssertionError("Wire-literal matching also removed neighboring text")


def _run(arguments: list[str], directory: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(  # noqa: S603
        arguments,
        cwd=directory,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )


def _pinned_tool(name: str, repository: str) -> str:
    executable = shutil.which(name)
    if executable is None:
        raise RuntimeError(f"Integration requires the pinned {name} executable already on PATH")
    hooks = ROOT.joinpath(".pre-commit-config.yaml").read_text(encoding="utf-8")
    pin = re.search(
        rf"(?m)^  - repo: https://github\.com/{re.escape(repository)}\n"
        r"    rev: v(\d+\.\d+\.\d+)$",
        hooks,
    )
    if pin is None:
        raise AssertionError(f"Could not read the {name} version pin")
    version = _run([executable, "--version"], ROOT)
    output = version.stdout + version.stderr
    if version.returncode != 0 or pin[1] not in output.split():
        raise RuntimeError(f"Integration requires {name} {pin[1]}, received {output.strip()!r}")
    return executable


def _test_tools() -> None:
    commands = (
        [
            _pinned_tool("typos", "crate-ci/typos"),
            "--isolated",
            "--config",
            str(ROOT / ".typos.toml"),
        ],
        [
            _pinned_tool("codespell", "codespell-project/codespell"),
            "--config",
            str(ROOT / ".codespellrc"),
        ],
    )
    failures = (
        (FRAGMENT, FRAGMENT),
        (f'"tag={FRAGMENT}"', FRAGMENT),
        (f'"tag={FRAGMENT}%G3"', FRAGMENT),
        (f'"tag={FRAGMENT}%A"', FRAGMENT),
        (f"{MISSPELLING} {ENCODED}", MISSPELLING),
        (f"{ENCODED} {MISSPELLING}", MISSPELLING),
    )
    with tempfile.TemporaryDirectory(prefix="wire-spelling-") as directory:
        fixture = Path(directory) / "fixture.rs"
        for command in commands:
            fixture.write_text(f"let value = {ENCODED};\n", encoding="utf-8")
            result = _run([*command, str(fixture)], fixture.parent)
            if result.returncode != 0:
                raise AssertionError(f"Wire literal failed: {result.stdout}{result.stderr}")
            for text, expected in failures:
                fixture.write_text(f"// {text}\n", encoding="utf-8")
                result = _run([*command, str(fixture)], fixture.parent)
                if result.returncode == 0 or expected not in result.stdout + result.stderr:
                    raise AssertionError(
                        f"Expected {expected!r} from {command[0]}: {result.stdout}{result.stderr}",
                    )


def main() -> None:
    """
    Run configuration probes and, when requested, the real pinned spelling tools.
    """
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--integration", action="store_true")
    arguments = parser.parse_args()
    _test_configuration()
    print("Wire-literal configuration probes passed (no spelling tools invoked)")
    if arguments.integration:
        _test_tools()
        print("Pinned spelling-tool integration passed")


if __name__ == "__main__":
    main()
