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
Summarize lookup allocation events from an interpreted Heaptrack v3 stream on stdin.
"""

import json
import sys
from collections import Counter
from dataclasses import asdict
from dataclasses import dataclass
from typing import BinaryIO


__all__: tuple[str, ...] = ()


@dataclass
class Cost:
    allocations: int = 0
    allocated_bytes: int = 0
    live_bytes: int = 0
    peak_bytes: int = 0

    def update(self, size: int, *, allocating: bool) -> None:
        if allocating:
            self.allocations += 1
            self.allocated_bytes += size
            self.live_bytes += size
            self.peak_bytes = max(self.peak_bytes, self.live_bytes)
        else:
            self.live_bytes -= size
        if self.live_bytes < 0:
            raise ValueError("Trace frees more bytes than it allocates")


# Keep the event decoder together so index construction follows stream order
def summarize(stream: BinaryIO) -> dict[str, Cost]:  # noqa: C901, PLR0912, PLR0915
    strings = [""]
    frames: list[list[str]] = [[]]
    traces: list[set[str]] = [set()]
    allocations: list[tuple[int, set[str]]] = []
    outstanding: Counter[int] = Counter()
    costs: dict[str, Cost] = {}
    version = False
    for line in stream:
        fields = line.split()
        if not fields:
            continue
        kind = fields[0]
        if kind == b"v":
            if fields[1:] != [b"10500", b"3"]:
                raise ValueError("Expected interpreted Heaptrack 1.5.0 format v3")
            version = True
        elif not version:
            raise ValueError("Missing Heaptrack version header")
        elif kind == b"s":
            _, length, value = line.rstrip(b"\n").split(b" ", 2)
            if len(value) != int(length, 16):
                raise ValueError("Invalid string byte length")
            strings.append(value.decode("utf-8", errors="replace"))
        elif kind == b"i":
            frames.append([strings[int(value, 16)] for value in fields[3::3]])
        elif kind == b"t":
            ip, parent = (int(value, 16) for value in fields[1:])
            names = set(traces[parent])
            for function in frames[ip]:
                for marker in (
                    "initialize_currencies",
                    "setup",
                    "first_lookup",
                    "steady_lookup",
                ):
                    if "cache_xrate_profile" in function and marker in function:
                        names.add(marker)
                for marker in (
                    "build_quote_table",
                    "build_bar_quote_table",
                    "get_exchange_rate",
                ):
                    if marker in function:
                        names.add(marker)
            traces.append(names)
        elif kind == b"a":
            size, trace = (int(value, 16) for value in fields[1:])
            names = set(traces[trace])
            if "first_lookup" in names:
                names.discard("steady_lookup")
            if not names:
                names.add("other")
            names.add("process")
            allocations.append((size, names))
        elif kind in (b"+", b"-"):
            index = int(fields[1], 16)
            allocating = kind == b"+"
            outstanding[index] += 1 if allocating else -1
            if outstanding[index] < 0:
                raise ValueError("Unmatched free")
            size, names = allocations[index]
            for name in names:
                costs.setdefault(name, Cost()).update(size, allocating=allocating)
    if not version:
        raise ValueError("Empty Heaptrack stream")
    return costs


if __name__ == "__main__":
    sys.stdout.write(
        json.dumps(
            {name: asdict(cost) for name, cost in summarize(sys.stdin.buffer).items()},
            indent=2,
            sort_keys=True,
        )
        + "\n",
    )
