#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

python3 -B - "$REPO_ROOT" << 'PY'
from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path


TEST_ATTRIBUTE_PATTERN = re.compile(
    r"^[ \t]*#\s*\[\s*(?:(?:[A-Za-z_][A-Za-z0-9_]*)::)*(?:test|rstest|test_case)\b",
    re.MULTILINE,
)


def make_lines(path: Path) -> list[str]:
    lines = []
    pending = ""
    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.rstrip()
        if pending:
            line = f"{pending} {line.lstrip()}"
        if line.endswith("\\"):
            pending = line[:-1].rstrip()
            continue
        lines.append(line)
        pending = ""

    if pending:
        lines.append(pending)
    return lines


def make_words(lines: list[str], name: str) -> list[str]:
    pattern = re.compile(rf"^{re.escape(name)}\s*:?=\s*(.*)$")
    for line in lines:
        if match := pattern.match(line):
            return match.group(1).split()
    raise ValueError(f"Makefile does not define {name}")


def rust_sources(package: dict, package_roots: set[Path]) -> set[Path]:
    package_root = Path(package["manifest_path"]).parent
    nested_roots = package_roots - {package_root}
    sources = set()
    for current, dirnames, filenames in os.walk(package_root):
        current_path = Path(current)
        dirnames[:] = [
            dirname for dirname in dirnames if current_path / dirname not in nested_roots
        ]
        sources.update(current_path / filename for filename in filenames if filename.endswith(".rs"))
    return sources


root = Path(sys.argv[1]).resolve()
metadata_command = ["cargo", "metadata", "--no-deps", "--format-version", "1"]
try:
    result = subprocess.run(
        metadata_command,
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )
except subprocess.CalledProcessError as exc:
    sys.stderr.write(exc.stderr)
    raise SystemExit(exc.returncode) from exc

metadata = json.loads(result.stdout)
member_ids = set(metadata["workspace_members"])
workspace_packages = {
    package["name"]: package for package in metadata["packages"] if package["id"] in member_ids
}
workspace_members = set(workspace_packages)
package_roots = {
    Path(package["manifest_path"]).parent for package in workspace_packages.values()
}

lines = make_lines(root / "Makefile")
inventory_names = ("CORE_CRATES", "ADAPTER_CRATES", "NO_TEST_CRATES")
inventories = {name: make_words(lines, name) for name in inventory_names}
entries = [crate for crates in inventories.values() for crate in crates]
entry_counts = Counter(entries)

errors = []
duplicates = sorted(crate for crate, count in entry_counts.items() if count != 1)
if duplicates:
    errors.append(f"workspace crates listed more than once: {', '.join(duplicates)}")

listed_crates = set(entries)
missing_crates = sorted(workspace_members - listed_crates)
if missing_crates:
    errors.append(f"workspace crates missing from inventories: {', '.join(missing_crates)}")

unknown_crates = sorted(listed_crates - workspace_members)
if unknown_crates:
    errors.append(f"inventory entries not found in the workspace: {', '.join(unknown_crates)}")

for crate in inventories["NO_TEST_CRATES"]:
    package = workspace_packages.get(crate)
    if package is None:
        continue

    test_targets = sorted(
        target["name"] for target in package["targets"] if "test" in target["kind"]
    )
    if test_targets:
        errors.append(f"no-test crate {crate} defines test targets: {', '.join(test_targets)}")

    test_sources = sorted(
        source.relative_to(root).as_posix()
        for source in rust_sources(package, package_roots)
        if TEST_ATTRIBUTE_PATTERN.search(source.read_text(encoding="utf-8"))
    )
    if test_sources:
        errors.append(f"no-test crate {crate} contains Rust tests: {', '.join(test_sources)}")

if errors:
    print("Workspace test coverage check failed:", file=sys.stderr)
    for error in errors:
        print(f"  - {error}", file=sys.stderr)
    raise SystemExit(1)

print(f"Workspace test coverage is complete ({len(workspace_members)} workspace members)")
PY
