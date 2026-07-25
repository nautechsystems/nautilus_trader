#!/usr/bin/env python3
"""
Check Markdown files for unpadded GFM table delimiter rows.

MD060 enforces pipe alignment but not space padding inside delimiter cells. This catches
rows like |-----| that should be | ----- |.

"""

import re
import sys
from pathlib import Path


DELIMITER_RE = re.compile(r"^\|[-:\s|]+\|$")
UNPADDED_RE = re.compile(r"[|][-:]|[-:][|]")


def find_violations(path):
    """
    Yield (line_number, line_text) for each unpadded delimiter row.
    """
    with open(path, encoding="utf-8") as f:
        for lineno, line in enumerate(f, 1):
            stripped = line.rstrip()
            if DELIMITER_RE.match(stripped) and UNPADDED_RE.search(stripped):
                yield lineno, stripped


def main():
    paths = [Path(a) for a in sys.argv[1:] if a.endswith(".md")]

    violations = []
    for path in sorted(paths):
        for lineno, line in find_violations(path):
            violations.append((path, lineno, line))

    if violations:
        for path, lineno, line in violations:
            print(f"{path}:{lineno}: unpadded table delimiter: {line}")
        print(
            "\nPad delimiter cells with spaces (| ----- |, not |-----|)."
            "\nMD060 enforces pipe alignment but not delimiter padding.",
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
