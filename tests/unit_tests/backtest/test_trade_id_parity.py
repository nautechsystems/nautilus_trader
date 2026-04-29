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

import pytest


FNV_OFFSET_BASIS = 0xCBF29CE484222325
FNV_PRIME = 0x100000001B3
U64_MASK = 0xFFFFFFFFFFFFFFFF


def _fnv1a_trade_id_hash(venue: str, raw_id: int, ts_init_ns: int) -> int:
    h = FNV_OFFSET_BASIS
    parts = (
        venue.encode("ascii"),
        b"\x1f",
        raw_id.to_bytes(4, "little"),
        b"\x1f",
        ts_init_ns.to_bytes(8, "little"),
    )

    for part in parts:
        for byte in part:
            h ^= byte
            h = (h * FNV_PRIME) & U64_MASK
    return h


# These fixtures are mirrored in
# crates/execution/src/matching_engine/ids_generator.rs
# (test_generate_trade_id_matches_python_parity_fixture).
# If either side changes the hash scheme, both tests must be updated
# together so the matching engine's trade_id contract stays identical
# across the Rust and Python bindings.
@pytest.mark.parametrize(
    ("venue", "raw_id", "ts_init", "expected"),
    [
        pytest.param("BINANCE", 1, 0, "T-59d6cf33c843f0cc-001", id="zero"),
        pytest.param(
            "BINANCE",
            1,
            1_700_000_000_000_000_000,
            "T-5c080ffb681dc0d4-001",
            id="nanos",
        ),
        pytest.param(
            "SOMETHING_VERY_LONG_FOR_SAFETY",
            42,
            1_700_000_000_000_000_000,
            "T-2a2238c5cc0cbaf2-001",
            id="long_venue",
        ),
    ],
)
def test_trade_id_format_matches_rust_parity_fixture(
    venue: str,
    raw_id: int,
    ts_init: int,
    expected: str,
) -> None:
    h = _fnv1a_trade_id_hash(venue, raw_id, ts_init)
    # Mirror the Cython format in OrderMatchingEngine._generate_trade_id_str
    # with an execution_count of 1.
    actual = f"T-{h:016x}-{1:03d}"
    assert actual == expected


def test_trade_id_multi_tick_counter_matches_rust_parity_fixture() -> None:
    # Four consecutive calls at the same ts (e.g. bar O/H/L/C) must produce
    # the sequence `-001`, `-002`, `-003`, `-004`. Mirrors
    # `test_generate_trade_id_multi_tick_matches_python_parity_fixture` in
    # crates/execution/src/matching_engine/ids_generator.rs.
    venue = "BINANCE"
    raw_id = 1
    ts_init = 1_700_000_000_000_000_000
    h = _fnv1a_trade_id_hash(venue, raw_id, ts_init)
    actual = [f"T-{h:016x}-{count:03d}" for count in range(1, 5)]
    expected = [
        "T-5c080ffb681dc0d4-001",
        "T-5c080ffb681dc0d4-002",
        "T-5c080ffb681dc0d4-003",
        "T-5c080ffb681dc0d4-004",
    ]
    assert actual == expected


def test_fnv1a_each_input_changes_the_hash() -> None:
    base = _fnv1a_trade_id_hash("BINANCE", 1, 1_700_000_000_000_000_000)
    assert base != _fnv1a_trade_id_hash("BYBIT", 1, 1_700_000_000_000_000_000)
    assert base != _fnv1a_trade_id_hash("BINANCE", 2, 1_700_000_000_000_000_000)
    assert base != _fnv1a_trade_id_hash("BINANCE", 1, 1_700_000_000_000_000_001)


def test_fnv1a_is_bounded_to_u64() -> None:
    # Pathological input should still produce a value representable in 16 hex chars.
    h = _fnv1a_trade_id_hash("X" * 50, 2**32 - 1, 2**64 - 1)
    assert 0 <= h <= U64_MASK
    assert len(f"{h:016x}") == 16
