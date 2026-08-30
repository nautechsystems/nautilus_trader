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
Report candidate strict Clippy diagnostics without making findings fail the command.

Cargo failures and malformed diagnostic output still fail so a successful report is
trustworthy.

"""

from __future__ import annotations

import argparse
import json
import shlex
import subprocess
import sys
import tomllib
from collections import Counter
from dataclasses import dataclass
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
LINTS = (
    "arithmetic_side_effects",
    "as_conversions",
    "expect_used",
    "indexing_slicing",
    "unwrap_used",
    "panic",
    "string_slice",
    "unreachable",
    "todo",
    "unimplemented",
    "exit",
    "panic_in_result_fn",
    "unchecked_time_subtraction",
)
LINT_SET = frozenset(LINTS)


class _AuditError(RuntimeError):
    pass


@dataclass(frozen=True, order=True)
class _Finding:
    package: str
    lint: str
    path: str
    line: int
    column: int


@dataclass(frozen=True)
class _ReportConfig:
    toolchain: str
    features: str
    profile: str
    production_command: tuple[str, ...]
    test_command: tuple[str, ...]


def _cargo_metadata(cargo: str) -> dict[str, str]:
    command = [cargo, "metadata", "--format-version", "1", "--no-deps", "--locked"]
    result = subprocess.run(  # noqa: S603
        command,
        cwd=REPO_ROOT,
        capture_output=True,
        check=False,
        encoding="utf-8",
    )
    if result.returncode != 0:
        detail = result.stderr.strip()
        raise _AuditError(detail or f"Cargo metadata failed with status {result.returncode}")

    try:
        metadata = json.loads(result.stdout)
    except json.JSONDecodeError as e:
        raise _AuditError(f"Cargo metadata returned invalid JSON: {e}") from e

    packages = metadata.get("packages")
    if not isinstance(packages, list):
        raise _AuditError("Cargo metadata did not contain a package list")

    names = {}
    for package in packages:
        if not isinstance(package, dict):
            raise _AuditError("Cargo metadata contained an invalid package entry")
        package_id = package.get("id")
        name = package.get("name")
        if not isinstance(package_id, str) or not isinstance(name, str):
            raise _AuditError("Cargo metadata contained a package without an ID and name")
        names[package_id] = name

    return names


def _clippy_command(
    cargo: str,
    features: str,
    profile: str,
    *,
    include_tests: bool,
) -> tuple[str, ...]:
    command = [
        cargo,
        "clippy",
        "--quiet",
        "--workspace",
        "--locked",
        "--lib",
        "--bins",
    ]
    if include_tests:
        command.append("--tests")
    command.extend(
        [
            "--features",
            features,
            "--profile",
            profile,
            "--no-deps",
            "--color",
            "never",
            "--message-format=json",
            "--",
        ],
    )
    for lint in LINTS:
        command.extend(("--force-warn", f"clippy::{lint}"))
    return tuple(command)


def _source_path(file_name: str) -> str:
    path = Path(file_name)
    if path.is_absolute():
        try:
            return path.relative_to(REPO_ROOT).as_posix()
        except ValueError:
            pass
    return path.as_posix()


# Keep the validation in one pass so malformed records fail with their lint context
def _parse_message(  # noqa: C901, PLR0912
    line: str,
    package_names: dict[str, str],
) -> set[_Finding]:
    try:
        payload = json.loads(line)
    except json.JSONDecodeError as e:
        raise _AuditError(f"Cargo Clippy returned invalid JSON: {e}") from e

    if not isinstance(payload, dict) or payload.get("reason") != "compiler-message":
        return set()

    message = payload.get("message")
    if not isinstance(message, dict):
        raise _AuditError("Cargo Clippy returned a compiler message without diagnostic data")

    code = message.get("code")
    if not isinstance(code, dict):
        return set()
    lint_code = code.get("code")
    if not isinstance(lint_code, str) or not lint_code.startswith("clippy::"):
        return set()
    lint = lint_code.removeprefix("clippy::")
    if lint not in LINT_SET:
        return set()

    package_id = payload.get("package_id")
    if not isinstance(package_id, str):
        raise _AuditError(f"Diagnostic for clippy::{lint} did not identify its package")
    try:
        package = package_names[package_id]
    except KeyError as e:
        raise _AuditError(f"Diagnostic for clippy::{lint} named an unknown package") from e

    spans = message.get("spans")
    if not isinstance(spans, list):
        raise _AuditError(f"Diagnostic for clippy::{lint} did not contain spans")

    findings = set()
    for span in spans:
        if not isinstance(span, dict) or span.get("is_primary") is not True:
            continue
        file_name = span.get("file_name")
        line = span.get("line_start")
        column = span.get("column_start")
        if (
            not isinstance(file_name, str)
            or not isinstance(line, int)
            or not isinstance(column, int)
        ):
            raise _AuditError(f"Diagnostic for clippy::{lint} contained an invalid primary span")
        findings.add(_Finding(package, lint, _source_path(file_name), line, column))

    if not findings:
        raise _AuditError(f"Diagnostic for clippy::{lint} did not contain a primary span")
    return findings


def _run_clippy(
    command: tuple[str, ...],
    label: str,
    package_names: dict[str, str],
) -> set[_Finding]:
    sys.stderr.write(f"Running {label}: {shlex.join(command)}\n")
    findings = set()

    with subprocess.Popen(  # noqa: S603
        command,
        cwd=REPO_ROOT,
        stdout=subprocess.PIPE,
        encoding="utf-8",
    ) as process:
        if process.stdout is None:
            raise _AuditError(f"{label} Cargo Clippy output was unavailable")
        try:
            for line in process.stdout:
                if line.strip():
                    findings.update(_parse_message(line, package_names))
        except _AuditError:
            process.kill()
            process.wait()
            raise

        return_code = process.wait()

    if return_code != 0:
        raise _AuditError(f"{label} Cargo Clippy run failed with status {return_code}")
    return findings


def _toolchain_channel() -> str:
    try:
        with (REPO_ROOT / "rust-toolchain.toml").open("rb") as file:
            config = tomllib.load(file)
        channel = config["toolchain"]["channel"]
    except (KeyError, OSError, tomllib.TOMLDecodeError) as e:
        raise _AuditError(f"Could not read the pinned Rust toolchain: {e}") from e
    if not isinstance(channel, str):
        raise _AuditError("The pinned Rust toolchain channel is not a string")
    return channel


def _summary_table(
    production: set[_Finding],
    tests: set[_Finding],
    full: set[_Finding],
) -> list[str]:
    production_counts = Counter(finding.lint for finding in production)
    test_counts = Counter(finding.lint for finding in tests)
    full_counts = Counter(finding.lint for finding in full)
    lines = [
        "| Lint | Production | Test-only | Full |",
        "| --- | ---: | ---: | ---: |",
    ]
    lines.extend(
        f"| `{lint}` | {production_counts[lint]:,} | {test_counts[lint]:,} | "
        f"{full_counts[lint]:,} |"
        for lint in LINTS
    )
    lines.append(
        f"| **Total** | **{len(production):,}** | **{len(tests):,}** | **{len(full):,}** |",
    )
    return lines


def _package_lint_table(findings: set[_Finding]) -> list[str]:
    counts = Counter((finding.package, finding.lint) for finding in findings)
    if not counts:
        return ["No findings."]

    lines = [
        "| Package | Lint | Count |",
        "| --- | --- | ---: |",
    ]
    lines.extend(
        f"| `{package}` | `{lint}` | {count:,} |"
        for (package, lint), count in sorted(counts.items())
    )
    return lines


def _render_report(
    production: set[_Finding],
    full_run: set[_Finding],
    config: _ReportConfig,
) -> str:
    tests = full_run - production
    full = full_run | production
    lines = [
        "# Strict Clippy audit",
        "",
        "Findings do not affect the command status. Cargo and report failures do.",
        "",
        "## Configuration",
        "",
        f"* Rust toolchain: `{config.toolchain}`",
        f"* Cargo profile: `{config.profile}`",
        f"* Features: `{config.features}`",
        "* Locations are deduplicated by package, lint, path, line, and column.",
        "* Test-only findings are full-run locations that do not occur in the production run.",
        "",
        "```bash",
        shlex.join(config.production_command),
        shlex.join(config.test_command),
        "```",
        "",
        "## Summary",
        "",
        *_summary_table(production, tests, full),
        "",
        "## Production by package and lint",
        "",
        *_package_lint_table(production),
        "",
        "## Test-only by package and lint",
        "",
        *_package_lint_table(tests),
    ]
    return "\n".join(lines)


def _parse_args(argv: list[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--features", required=True, help="Comma-separated workspace features")
    parser.add_argument("--profile", required=True, help="Cargo build profile")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """
    Run the production and test audits and render their Markdown report.
    """
    args = _parse_args(argv)
    if not args.features.strip():
        raise _AuditError("At least one Cargo feature is required")
    if not args.profile.strip():
        raise _AuditError("A Cargo profile is required")

    cargo = "cargo"
    toolchain = _toolchain_channel()
    package_names = _cargo_metadata(cargo)
    production_command = _clippy_command(
        cargo,
        args.features,
        args.profile,
        include_tests=False,
    )
    test_command = _clippy_command(
        cargo,
        args.features,
        args.profile,
        include_tests=True,
    )
    production = _run_clippy(production_command, "production", package_names)
    full_run = _run_clippy(test_command, "test", package_names)
    report = _render_report(
        production,
        full_run,
        _ReportConfig(
            toolchain=toolchain,
            features=args.features,
            profile=args.profile,
            production_command=production_command,
            test_command=test_command,
        ),
    )
    sys.stdout.write(f"{report}\n")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except _AuditError as e:
        sys.stderr.write(f"Strict Clippy audit failed: {e}\n")
        raise SystemExit(1) from e
