#!/usr/bin/env python3
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License, Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software distributed under the
#  License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either
#  express or implied. See the License for the specific language governing permissions and
#  limitations under the License.
"""
Render or check vendored pre-commit definitions in a consumer config.
"""

from __future__ import annotations

import argparse
import pathlib
import shutil
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path


BEGIN_MARKER = "  # nautilus-engineering: begin"
END_MARKER = "  # nautilus-engineering: end"
DEFAULT_CONFIG = ".pre-commit-config.yaml"
DEFAULT_LOCK = ".nautilus-engineering.lock"
FRAGMENT_DIRECTORY = ".nautilus-engineering/pre-commit"


class _ManagedSectionError(Exception):
    pass


@dataclass(frozen=True)
class _Fragment:
    artifact: str
    path: str
    content: str


def _git_executable() -> str:
    executable = shutil.which("git")
    if executable is None:
        raise _ManagedSectionError("git was not found on PATH")
    return str(Path(executable).resolve())


def _run_git(root: Path, *args: str) -> bytes:
    process = subprocess.run(  # noqa: S603
        [_git_executable(), "-C", str(root), *args],
        capture_output=True,
        check=False,
    )
    if process.returncode != 0:
        detail = process.stderr.decode("utf-8", errors="replace").strip()
        raise _ManagedSectionError(detail or f"git {' '.join(args)} failed")
    return process.stdout


def _repository_root() -> Path:
    root = _run_git(Path.cwd(), "rev-parse", "--show-toplevel")
    return Path(root.decode("utf-8").strip()).resolve()


def _validate_path(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise _ManagedSectionError(f"{label} must be a non-empty string")
    path = pathlib.PurePosixPath(value)
    if (
        path.is_absolute()
        or str(path) != value
        or any(part in ("", ".", "..") for part in path.parts)
    ):
        raise _ManagedSectionError(
            f"{label} must be a normalized repository-relative path: {value}",
        )
    if any(part.casefold() == ".git" for part in path.parts):
        raise _ManagedSectionError(f"{label} contains a reserved path component: {value}")
    return value


def _reject_symlink_path(root: Path, relative: str) -> None:
    current = root
    for part in pathlib.PurePosixPath(relative).parts:
        current /= part
        if current.is_symlink():
            raise _ManagedSectionError(f"path traverses a symlink: {relative}")


def _read_file(root: Path, relative: str, *, staged: bool) -> bytes:
    _validate_path(relative, "path")
    if staged:
        return _run_git(root, "show", f":{relative}")
    _reject_symlink_path(root, relative)
    path = root.joinpath(*pathlib.PurePosixPath(relative).parts)
    if not path.is_file():
        raise _ManagedSectionError(f"file not found: {relative}")
    return path.read_bytes()


def _decode_file(content: bytes, relative: str) -> str:
    try:
        text = content.decode("utf-8")
    except UnicodeDecodeError as e:
        raise _ManagedSectionError(f"file is not UTF-8: {relative}") from e
    if "\r" in text:
        raise _ManagedSectionError(f"file must use LF line endings: {relative}")
    if not text.endswith("\n"):
        raise _ManagedSectionError(f"file must end with a newline: {relative}")
    return text


def _load_fragments(root: Path, lock_path: str, *, staged: bool) -> list[_Fragment]:
    raw_lock = _read_file(root, lock_path, staged=staged)
    try:
        lock = tomllib.loads(raw_lock.decode("utf-8"))
    except (UnicodeDecodeError, tomllib.TOMLDecodeError) as e:
        raise _ManagedSectionError(f"invalid sync lock: {e}") from e

    files = lock.get("file")
    if not isinstance(files, list):
        raise _ManagedSectionError("sync lock has no file entries")

    fragments: list[_Fragment] = []
    artifacts: set[str] = set()
    for entry in files:
        if not isinstance(entry, dict):
            raise _ManagedSectionError("sync lock contains an invalid file entry")
        artifact = entry.get("artifact")
        path = _validate_path(entry.get("path"), "locked path")
        if not isinstance(artifact, str):
            raise _ManagedSectionError("sync lock contains an invalid artifact id")
        if artifact in artifacts:
            raise _ManagedSectionError(f"sync lock repeats artifact: {artifact}")
        artifacts.add(artifact)
        parent = str(pathlib.PurePosixPath(path).parent)
        if not artifact.startswith("pre-commit-") or parent != FRAGMENT_DIRECTORY:
            continue
        content = _decode_file(_read_file(root, path, staged=staged), path)
        fragments.append(_Fragment(artifact, path, content))

    if not fragments:
        raise _ManagedSectionError("sync lock selects no pre-commit definition fragments")
    return sorted(fragments, key=lambda fragment: fragment.artifact)


def _indent_fragment(fragment: _Fragment) -> list[str]:
    lines = fragment.content[:-1].split("\n")
    if not lines or not lines[0].startswith("- repo:"):
        raise _ManagedSectionError(
            f"pre-commit fragment has an invalid first line: {fragment.path}",
        )
    return [f"  {line}" if line else "" for line in lines]


def _render_section(fragments: list[_Fragment]) -> list[str]:
    lines = [BEGIN_MARKER]
    for index, fragment in enumerate(fragments):
        if index:
            lines.append("")
        lines.extend(_indent_fragment(fragment))
    lines.append(END_MARKER)
    return lines


def _section_bounds(lines: list[str]) -> tuple[int, int] | None:
    begins = [index for index, line in enumerate(lines) if line == BEGIN_MARKER]
    ends = [index for index, line in enumerate(lines) if line == END_MARKER]
    if not begins and not ends:
        return None
    if len(begins) != 1 or len(ends) != 1 or begins[0] >= ends[0]:
        raise _ManagedSectionError("pre-commit config has invalid managed-section markers")
    return begins[0], ends[0]


def _find_lines(lines: list[str], target: list[str]) -> int | None:
    limit = len(lines) - len(target) + 1
    for index in range(max(limit, 0)):
        if lines[index : index + len(target)] == target:
            return index
    return None


def _remove_unmanaged_fragments(lines: list[str], fragments: list[_Fragment]) -> list[str]:
    updated = list(lines)
    for fragment in fragments:
        target = _indent_fragment(fragment)
        while (index := _find_lines(updated, target)) is not None:
            del updated[index : index + len(target)]
            if 0 < index < len(updated) and not updated[index - 1] and not updated[index]:
                del updated[index]
    return updated


def _fragment_identities(fragment: _Fragment) -> list[str]:
    lines = [line.strip() for line in fragment.content.splitlines()]
    identities: list[str] = []
    local = False
    repositories = 0
    for line in lines:
        if line.startswith("- repo:"):
            repositories += 1
            local = line == "- repo: local"
            if not local:
                identities.append(line)
        elif local and line.startswith("- id:"):
            identities.append(line)
    if repositories == 0:
        raise _ManagedSectionError(
            f"pre-commit fragment has no repository entry: {fragment.path}",
        )
    if not identities:
        raise _ManagedSectionError(
            f"pre-commit fragment has no repository or hook identity: {fragment.path}",
        )
    return identities


def _unmanaged_conflicts(lines: list[str], fragments: list[_Fragment]) -> list[str]:
    stripped = {line.strip() for line in lines}
    return [
        fragment.artifact
        for fragment in fragments
        if any(identity in stripped for identity in _fragment_identities(fragment))
    ]


def _config_lines(content: str) -> list[str]:
    if "\r" in content:
        raise _ManagedSectionError("pre-commit config must use LF line endings")
    if not content.endswith("\n"):
        raise _ManagedSectionError("pre-commit config must end with a newline")
    return content[:-1].split("\n")


def _render_config(content: str, fragments: list[_Fragment]) -> str:
    lines = _config_lines(content)
    bounds = _section_bounds(lines)
    if bounds is not None:
        start, end = bounds
        del lines[start : end + 1]

    lines = _remove_unmanaged_fragments(lines, fragments)
    conflicts = _unmanaged_conflicts(lines, fragments)
    if conflicts:
        raise _ManagedSectionError(
            f"pre-commit definitions conflict with the managed section: {', '.join(conflicts)}",
        )
    repos = [index for index, line in enumerate(lines) if line == "repos:"]
    if len(repos) != 1:
        raise _ManagedSectionError("pre-commit config must contain one top-level repos key")
    insertion = repos[0] + 1
    section = _render_section(fragments)
    if insertion < len(lines) and lines[insertion]:
        section.append("")
    lines[insertion:insertion] = section
    return "\n".join(lines) + "\n"


def _check_config(content: str, fragments: list[_Fragment]) -> None:
    lines = _config_lines(content)
    bounds = _section_bounds(lines)
    expected = _render_section(fragments)
    if bounds is None or lines[bounds[0] : bounds[1] + 1] != expected:
        raise _ManagedSectionError(
            "managed pre-commit section differs; run "
            "python3 scripts/manage-nautilus-engineering-pre-commit.py render",
        )

    outside = lines[: bounds[0]] + lines[bounds[1] + 1 :]
    conflicts = _unmanaged_conflicts(outside, fragments)
    if conflicts:
        raise _ManagedSectionError(
            "managed pre-commit definitions also appear outside the section: "
            f"{', '.join(conflicts)}",
        )


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    render = subparsers.add_parser("render", help="render the managed section in the worktree")
    check = subparsers.add_parser("check", help="check the managed section")
    for command in (render, check):
        command.add_argument("--config", default=DEFAULT_CONFIG, help="consumer pre-commit config")
        command.add_argument("--lock", default=DEFAULT_LOCK, help="consumer sync lock")
    check.add_argument("--staged", action="store_true", help="check staged Git blobs")
    return parser


def _write(message: str, *, error: bool = False) -> None:
    stream = sys.stderr if error else sys.stdout
    stream.write(f"{message}\n")


def _main() -> int:
    args = _build_parser().parse_args()
    try:
        root = _repository_root()
        staged = getattr(args, "staged", False)
        fragments = _load_fragments(root, args.lock, staged=staged)
        config_path = _validate_path(args.config, "config path")
        config = _decode_file(
            _read_file(root, config_path, staged=staged),
            config_path,
        )
        if args.command == "render":
            rendered = _render_config(config, fragments)
            path = root.joinpath(*pathlib.PurePosixPath(config_path).parts)
            if rendered != config:
                path.write_text(rendered, encoding="utf-8", newline="\n")
                _write(f"Updated managed pre-commit section in {config_path}")
            else:
                _write(f"Managed pre-commit section is current in {config_path}")
        else:
            _check_config(config, fragments)
            state = "staged " if args.staged else ""
            _write(f"Managed pre-commit section matches {state}vendored definitions")
    except (OSError, _ManagedSectionError) as e:
        _write(f"Error: {e}", error=True)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(_main())
