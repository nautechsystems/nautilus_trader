#!/usr/bin/env python3
"""
Normalize GFM table column widths and delimiter padding in Markdown files.

MD060 enforces pipe alignment but ignores cell content, so a column stays valid at any
width as long as every row agrees on it. This rewrites each column to its widest cell
plus one space either side, which also produces the padded delimiter cells (| ----- |,
not |-----|) that MD060 leaves unchecked.

Tables indented inside list items or blockquotes are out of scope, as are tables without
leading and trailing pipes.

"""

import re
import sys
from pathlib import Path


DELIMITER_RE = re.compile(r"^\|(?:\s*:?-+:?\s*\|)+$")
FENCE_RE = re.compile(r"^\s{0,3}(`{3,}|~{3,})\s*(.*)$")
PIPE_RE = re.compile(r"(?<!\\)\|")

MIN_DASHES = 3


def split_cells(row):
    """
    Return the cells of a table row, ignoring escaped pipes.
    """
    return PIPE_RE.split(row.rstrip())[1:-1]


def utf16_width(text):
    """
    Return the width markdownlint sees, which indexes strings by UTF-16 code unit.
    """
    return len(text) + sum(1 for char in text if ord(char) > 0xFFFF)


def find_tables(lines):
    """
    Yield (start, end) index ranges for pipe-table blocks outside code fences.
    """
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


def cell_alignment(cell):
    """
    Return how a cell pads its content, so normalization preserves the author's intent.
    """
    leading = len(cell) - len(cell.lstrip(" "))
    trailing = len(cell) - len(cell.rstrip(" "))
    if leading > 1 and trailing > 1:
        return "center"
    if leading > 1:
        return "right"
    return "left"


def render_cell(content, width, alignment):
    padding = width - 2 - utf16_width(content)
    if alignment == "right":
        return f" {' ' * padding}{content} "
    if alignment == "center":
        left = padding // 2
        return f" {' ' * left}{content}{' ' * (padding - left)} "
    return f" {content}{' ' * padding} "


def render_delimiter(marker, width):
    left = marker.startswith(":")
    right = marker.endswith(":")
    dashes = width - 2 - int(left) - int(right)
    body = f"{':' if left else ''}{'-' * dashes}{':' if right else ''}"
    return f" {body} "


def normalize_table(rows):
    """
    Return the rows rewritten to content width, or None when the block is not a table.
    """
    if len(rows) < 2 or not DELIMITER_RE.match(rows[1].rstrip()):
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


def normalize_file(path):
    """
    Rewrite the file's tables in place, returning True when anything changed.
    """
    lines = path.read_text(encoding="utf-8").split("\n")
    updated = list(lines)
    for start, end in find_tables(lines):
        normalized = normalize_table(lines[start:end])
        if normalized is not None:
            updated[start:end] = normalized

    if updated == lines:
        return False

    path.write_text("\n".join(updated), encoding="utf-8")
    return True


def main():
    paths = [Path(a) for a in sys.argv[1:] if a.endswith(".md")]

    changed = [path for path in sorted(paths) if normalize_file(path)]

    if changed:
        for path in changed:
            print(f"{path}: normalized table column widths")
        print(
            "\nTables are padded to the widest cell plus one space either side."
            "\nMD060 enforces pipe alignment but not column width.",
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
