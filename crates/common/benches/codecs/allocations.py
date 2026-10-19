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
Attribute heaptrack 1.5 allocation events to native codec benchmark cases.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
from pathlib import Path


# Record kinds share indexed tables and must retain their arrival order.
def _read_counts(  # noqa: C901
    path: Path,
    operation: str,
) -> tuple[list[int], list[int]]:
    executable = shutil.which("zstdcat")
    if executable is None:
        raise FileNotFoundError("zstdcat is required to read the heaptrack trace")

    strings = [""]
    frames: list[tuple[str, ...]] = [()]
    traces = [(False, False)]
    allocations: list[tuple[int, int]] = []
    counts: list[int] = []
    sizes: list[int] = []
    function = f"benchmarks6{operation}"

    with subprocess.Popen(  # noqa: S603 (resolved executable; arguments are paths, without a shell)
        [executable, "--", str(path)],
        stdout=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    ) as process:
        for line in process.stdout:
            kind, _, fields = line.rstrip("\n").partition(" ")
            match kind:
                case "s":
                    strings.append(fields.split(" ", 1)[1])
                case "i":
                    values = fields.split()
                    frames.append(
                        tuple(strings[int(values[i], 16)] for i in range(2, len(values), 3)),
                    )
                case "t":
                    ip, parent = (int(value, 16) for value in fields.split())
                    selected, marker = traces[parent]
                    traces.append(
                        (
                            selected or any(function in frame for frame in frames[ip]),
                            marker or any("profile_boundary" in frame for frame in frames[ip]),
                        ),
                    )
                case "a":
                    size, trace = (int(value, 16) for value in fields.split())
                    allocations.append((size, trace))
                case "+":
                    size, trace = allocations[int(fields, 16)]
                    selected, marker = traces[trace]
                    if marker:
                        if size != 1:
                            raise ValueError("unexpected codec profile marker size")
                        counts.append(0)
                        sizes.append(0)
                    elif selected:
                        if not counts:
                            raise ValueError("codec allocation precedes the first case marker")
                        counts[-1] += 1
                        sizes[-1] += size

        if process.wait() != 0:
            raise RuntimeError("zstdcat could not read the heaptrack trace")

    return counts, sizes


def _main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", type=Path, help="interpreted heaptrack .zst file")
    parser.add_argument("log", type=Path, help="matching native benchmark output")
    args = parser.parse_args()
    rows = [
        json.loads(line[line.index("{") :])
        for line in args.log.read_text(encoding="utf-8").splitlines()
        if '"profile_calls"' in line
    ]
    operations = {row["operation"] for row in rows}
    if len(operations) != 1 or not operations <= {"encode", "decode"}:
        parser.error("capture exactly one encode or decode operation")

    counts, sizes = _read_counts(args.trace, operations.pop())
    if len(rows) != len(counts):
        parser.error("case markers do not match the benchmark output")
    for row, count, size in zip(rows, counts, sizes, strict=True):
        calls = row["profile_calls"]
        if calls <= 0:
            parser.error("profile_calls must be positive")
        row["allocations"] = count / calls
        row["allocated_bytes"] = size / calls
        sys.stdout.write(json.dumps(row) + "\n")


if __name__ == "__main__":
    _main()
