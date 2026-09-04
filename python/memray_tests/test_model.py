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
Test retained allocations from Rust-backed model object workloads.
"""

import gc
import pickle

import pytest

from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick


pytest.importorskip("pytest_memray")

_CONSTRUCTION_RUNS = 20_000
_SERIALIZATION_RUNS = 5_000
_INSTRUMENT_ID = InstrumentId.from_str("AUD/USD.SIM")
_BID_PRICE = Price.from_str("1.00000")
_ASK_PRICE = Price.from_str("1.00001")
_BID_SIZE = Quantity.from_int(500_000)
_ASK_SIZE = Quantity.from_int(800_000)


@pytest.fixture(scope="module", autouse=True)
def _warm_up_model_workloads() -> None:
    _construct_quotes(1)
    _roundtrip_quotes(1)
    _pickle_quotes(1)


@pytest.mark.limit_leaks("32 KB")
def test_quote_construction_does_not_retain_native_allocations() -> None:
    """
    Test repeated Rust-backed quote construction and property access.
    """
    assert _construct_quotes(_CONSTRUCTION_RUNS) == _CONSTRUCTION_RUNS - 1
    gc.collect()


@pytest.mark.limit_leaks("32 KB")
def test_quote_serialization_does_not_retain_native_allocations() -> None:
    """
    Test dict, JSON, and MessagePack round trips across the PyO3 boundary.
    """
    assert _roundtrip_quotes(_SERIALIZATION_RUNS) == _SERIALIZATION_RUNS - 1
    gc.collect()


@pytest.mark.limit_leaks("32 KB")
def test_quote_pickle_does_not_retain_native_allocations() -> None:
    """
    Test repeated pickle round trips release temporary native allocations.
    """
    assert _pickle_quotes(_SERIALIZATION_RUNS) == _SERIALIZATION_RUNS - 1
    gc.collect()


def _construct_quotes(runs: int) -> int:
    last_ts = 0

    for ts in range(runs):
        quote = _quote(ts)
        assert quote.instrument_id == _INSTRUMENT_ID
        assert quote.bid_price == _BID_PRICE
        assert quote.ask_price == _ASK_PRICE
        assert quote.bid_size == _BID_SIZE
        assert quote.ask_size == _ASK_SIZE
        assert repr(quote) == f"QuoteTick(AUD/USD.SIM,1.00000,1.00001,500000,800000,{ts})"
        last_ts = quote.ts_init

    return last_ts


def _roundtrip_quotes(runs: int) -> int:
    last_ts = 0

    for ts in range(runs):
        quote = _quote(ts)
        from_dict = QuoteTick.from_dict(quote.to_dict())
        from_json = QuoteTick.from_json(quote.to_json_bytes())
        from_msgpack = QuoteTick.from_msgpack(quote.to_msgpack_bytes())

        assert (from_dict, from_json, from_msgpack) == (quote, quote, quote)
        last_ts = from_msgpack.ts_init

    return last_ts


def _pickle_quotes(runs: int) -> int:
    last_ts = 0

    for ts in range(runs):
        quote = _quote(ts)
        restored = pickle.loads(pickle.dumps(quote))  # noqa: S301 (trusted test data)

        assert restored == quote
        last_ts = restored.ts_init

    return last_ts


def _quote(ts: int) -> QuoteTick:
    return QuoteTick(
        instrument_id=_INSTRUMENT_ID,
        bid_price=_BID_PRICE,
        ask_price=_ASK_PRICE,
        bid_size=_BID_SIZE,
        ask_size=_ASK_SIZE,
        ts_event=ts,
        ts_init=ts,
    )
