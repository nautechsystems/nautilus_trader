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
Test the exchange-rate Heaptrack event accounting.
"""

import importlib.util
import io
import sys
from dataclasses import asdict
from pathlib import Path

import pytest


spec = importlib.util.spec_from_file_location(
    "profile_xrate_heap",
    Path(__file__).resolve().parents[3] / "scripts/profile-xrate-heap.py",
)
profile = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = profile
spec.loader.exec_module(profile)


def test_counts_exclude_setup_and_peak_tracks_simultaneous_allocations() -> None:
    """
    Keep setup, first-use, cumulative, and simultaneous allocation costs distinct.
    """
    names = [
        "cache_xrate_profile::setup",
        "cache_xrate_profile::steady_lookup",
        "cache_xrate_profile::first_lookup",
    ]
    strings = "".join(f"s {len(name):x} {name}\n" for name in names)
    stream = (
        "v 10500 3\n"
        + strings
        + """i 10 0 1
i 20 0 2
i 30 0 3
t 1 0
t 2 0
t 3 2
a 10 1
a 20 3
a 30 2
+ 0
+ 1
- 1
+ 2
- 2
+ 2
- 2
- 0
"""
    )
    result = {
        name: asdict(cost) for name, cost in profile.summarize(io.BytesIO(stream.encode())).items()
    }

    assert result == {
        "setup": {"allocations": 1, "allocated_bytes": 16, "live_bytes": 0, "peak_bytes": 16},
        "first_lookup": {
            "allocations": 1,
            "allocated_bytes": 32,
            "live_bytes": 0,
            "peak_bytes": 32,
        },
        "steady_lookup": {
            "allocations": 2,
            "allocated_bytes": 96,
            "live_bytes": 0,
            "peak_bytes": 48,
        },
        "process": {"allocations": 4, "allocated_bytes": 144, "live_bytes": 0, "peak_bytes": 64},
    }


@pytest.mark.parametrize(
    ("stream", "message"),
    [
        (b"", "Empty Heaptrack stream"),
        (b"v 10500 2\n", "Expected interpreted Heaptrack"),
        (b"v 10500 3\ns 4 abc\n", "Invalid string byte length"),
        (b"v 10500 3\nt 0 0\na 10 1\n- 0\n", "Unmatched free"),
    ],
)
def test_rejects_invalid_accounting_input(stream: bytes, message: str) -> None:
    """
    Reject unsupported formats and invalid allocation records.
    """
    with pytest.raises(ValueError, match=message):
        profile.summarize(io.BytesIO(stream))


def test_retained_allocations_and_inline_frames() -> None:
    """
    Count live allocations sharing a trace and recognize inlined lookup frames.
    """
    name = "cache_xrate_profile::steady_lookup"
    stream = (
        f"v 10500 3\ns 1 x\ns {len(name):x} {name}\n"
        """i 10 0 1 0 0 2 0 0
t 1 0
a 8 1
+ 0
+ 0
- 0
"""
    )
    result = profile.summarize(io.BytesIO(stream.encode()))

    assert result == {
        "steady_lookup": profile.Cost(2, 16, 8, 16),
        "process": profile.Cost(2, 16, 8, 16),
    }
