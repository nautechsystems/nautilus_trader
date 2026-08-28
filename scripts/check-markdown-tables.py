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
"""Normalize GFM table column widths and delimiter padding in Markdown files."""

import re
import sys
from collections.abc import Iterator
from pathlib import Path


__all__: tuple[str, ...] = ()

DELIMITER_RE = re.compile(r"^\|(?:\s*:?-+:?\s*\|)+$")
FENCE_RE = re.compile(r"^\s{0,3}(`{3,}|~{3,})\s*(.*)$")
PIPE_RE = re.compile(r"(?<!\\)\|")

MIN_DASHES = 3
MIN_TABLE_ROWS = 2
UTF16_BMP_MAX = 0xFFFF


def split_cells(row: str) -> list[str]:
    return PIPE_RE.split(row.rstrip())[1:-1]


def utf16_width(value: str) -> int:
    return len(value) + sum(1 for character in value if ord(character) > UTF16_BMP_MAX)


def find_tables(lines: list[str]) -> Iterator[tuple[int, int]]:
    fence = None
    index = 0
    while index < len(lines):
        match = FENCE_RE.match(lines[index])
        if match:
            marker, trailing = match.groups()
            if fence is None:
                fence = marker
            elif marker[0] == fence[0] and len(marker) >= len(fence) and not trailing:
                fence = None
            index += 1
            continue
        if fence is not None or not lines[index].startswith("|"):
            index += 1
            continue
        start = index
        while index < len(lines) and lines[index].startswith("|"):
            index += 1
        yield start, index


def cell_alignment(cell: str) -> str:
    leading = len(cell) - len(cell.lstrip(" "))
    trailing = len(cell) - len(cell.rstrip(" "))
    if leading > 1 and trailing > 1:
        return "center"
    if leading > 1:
        return "right"
    return "left"


def render_cell(content: str, width: int, alignment: str) -> str:
    padding = width - 2 - utf16_width(content)
    if alignment == "right":
        return f" {' ' * padding}{content} "
    if alignment == "center":
        left = padding // 2
        return f" {' ' * left}{content}{' ' * (padding - left)} "
    return f" {content}{' ' * padding} "


def render_delimiter(marker: str, width: int) -> str:
    left = marker.startswith(":")
    right = marker.endswith(":")
    dashes = width - 2 - int(left) - int(right)
    body = f"{':' if left else ''}{'-' * dashes}{':' if right else ''}"
    return f" {body} "


def normalize_table(rows: list[str]) -> list[str] | None:
    if len(rows) < MIN_TABLE_ROWS or not DELIMITER_RE.match(rows[1].rstrip()):
        return None

    grid = [split_cells(row) for row in rows]
    columns = len(grid[0])
    if columns == 0 or any(len(cells) != columns for cells in grid):
        return None

    widths = []
    for column in range(columns):
        marker = grid[1][column].strip()
        colons = int(marker.startswith(":")) + int(marker.endswith(":"))
        content = max(
            utf16_width(cells[column].strip()) for index, cells in enumerate(grid) if index != 1
        )
        widths.append(max(content, MIN_DASHES + colons) + 2)

    normalized = []
    for index, cells in enumerate(grid):
        if index == 1:
            rendered = [
                render_delimiter(cells[column].strip(), widths[column]) for column in range(columns)
            ]
        else:
            rendered = [
                render_cell(
                    cells[column].strip(),
                    widths[column],
                    cell_alignment(cells[column]),
                )
                for column in range(columns)
            ]
        normalized.append(f"|{'|'.join(rendered)}|")
    return normalized


def normalize_file(path: Path) -> bool:
    lines = path.read_text(encoding="utf-8").split("\n")
    updated = list(lines)
    for start, end in find_tables(lines):
        normalized = normalize_table(lines[start:end])
        if normalized is not None:
            updated[start:end] = normalized

    if updated == lines:
        return False

    path.write_text("\n".join(updated), encoding="utf-8", newline="\n")
    return True


def _write(message: str) -> None:
    sys.stdout.write(f"{message}\n")


def main() -> int:
    paths = [Path(argument) for argument in sys.argv[1:] if argument.endswith(".md")]
    changed = [path for path in sorted(paths) if normalize_file(path)]

    if changed:
        for path in changed:
            _write(f"{path}: normalized table column widths")
        _write(
            "\nTables are padded to the widest cell plus one space either side."
            "\nMD060 enforces pipe alignment but not column width.",
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
