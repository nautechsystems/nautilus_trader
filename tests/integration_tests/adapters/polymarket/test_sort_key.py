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

from nautilus_trader.core.nautilus_pyo3 import polymarket_trade_id
from nautilus_trader.core.nautilus_pyo3 import polymarket_trade_sort_key


def test_sort_key_returns_expected_tuple_for_canonical_record():
    # Arrange
    trade = {
        "timestamp": 1729000000,
        "transactionHash": "0xdeadbeef",
        "asset": "12345",
        "side": "BUY",
        "price": 0.5,
        "size": 10.0,
    }

    # Act
    key = polymarket_trade_sort_key(trade)

    # Assert - field order: (timestamp, transactionHash, asset, side, price, size)
    assert key == (1729000000, "0xdeadbeef", "12345", "BUY", "0.5", "10.0")


def test_sort_key_missing_keys_default_to_empty_strings_and_zero():
    # Arrange - matches the loader's prior `dict.get(key, "")` semantics
    trade: dict = {}

    # Act
    key = polymarket_trade_sort_key(trade)

    # Assert
    assert key == (0, "", "", "", "", "")


def test_sort_key_stringifies_numeric_price_and_size():
    # Arrange - mirror Python's str(value) used by the legacy sort key
    trade = {"timestamp": 1, "price": 0.123456, "size": 100}

    # Act
    key = polymarket_trade_sort_key(trade)

    # Assert
    assert key[4] == "0.123456"
    assert key[5] == "100"


def test_sort_key_sorts_pages_deterministically():
    # Arrange - mixed-page Polymarket Data API trades sharing a timestamp
    pages = [
        {
            "timestamp": 1729000000,
            "transactionHash": "0xC",
            "asset": "T",
            "side": "BUY",
            "price": 0.5,
            "size": 1.0,
        },
        {
            "timestamp": 1729000000,
            "transactionHash": "0xA",
            "asset": "T",
            "side": "SELL",
            "price": 0.5,
            "size": 1.0,
        },
        {
            "timestamp": 1729000000,
            "transactionHash": "0xB",
            "asset": "T",
            "side": "BUY",
            "price": 0.5,
            "size": 1.0,
        },
        {
            "timestamp": 1729000005,
            "transactionHash": "0xZ",
            "asset": "T",
            "side": "BUY",
            "price": 0.5,
            "size": 1.0,
        },
    ]

    # Act
    pages.sort(key=polymarket_trade_sort_key)

    # Assert - ascending by timestamp first, then transactionHash
    assert [t["transactionHash"] for t in pages] == ["0xA", "0xB", "0xC", "0xZ"]


def test_sort_key_field_order_matches_legacy_python_tuple():
    # Arrange - lock the field-order contract so a Rust-side reorder does
    # not silently break Python sorts that depend on the tuple shape.
    trade = {
        "timestamp": 7,
        "transactionHash": "tx",
        "asset": "asset",
        "side": "BUY",
        "price": 0.3,
        "size": 2,
    }
    expected_legacy = (
        int(trade["timestamp"]),
        str(trade.get("transactionHash", "")),
        str(trade.get("asset", "")),
        str(trade.get("side", "")),
        str(trade.get("price", "")),
        str(trade.get("size", "")),
    )

    # Act
    key = polymarket_trade_sort_key(trade)

    # Assert
    assert key == expected_legacy


@pytest.mark.parametrize(
    "value",
    ["not-an-int", 1.5, None, [1, 2]],
)
def test_sort_key_rejects_non_integer_timestamp(value):
    # Arrange
    trade = {"timestamp": value}

    # Act + Assert - pyo3 should refuse to coerce; the loader emits ints
    # when reading the venue payload, so any other type is a programmer error
    with pytest.raises((TypeError, ValueError)):
        polymarket_trade_sort_key(trade)


def test_trade_id_format_with_long_inputs():
    # Arrange
    tx_hash = "0x" + ("ab" * 32)  # 66 chars total
    asset = "12345678901234567890"
    seq = 0

    # Act
    trade_id = polymarket_trade_id(tx_hash, asset, seq)

    # Assert - last 24 of hash, last 4 of asset, zero-padded 6-digit seq
    assert trade_id == f"{tx_hash[-24:]}-{asset[-4:]}-000000"


def test_trade_id_pads_short_inputs():
    # Arrange - shorter than the slice cutoffs; should use the whole strings
    tx_hash = "0xabcd"
    asset = "Yes"
    seq = 7

    # Act
    trade_id = polymarket_trade_id(tx_hash, asset, seq)

    # Assert
    assert trade_id == "0xabcd-Yes-000007"


def test_trade_id_increments_sequence_format():
    # Sanity check that callers can rely on monotonic, lex-comparable suffixes
    ids = [polymarket_trade_id("0xhash", "0xasset", i) for i in range(3)]
    assert ids == sorted(ids)
    assert ids[0].endswith("-000000")
    assert ids[2].endswith("-000002")
