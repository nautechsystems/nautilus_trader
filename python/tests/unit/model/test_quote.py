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
Test quote behavior.
"""

import pickle
import re

import pytest

from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import PriceType
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick


@pytest.fixture
def quote(audusd_id: InstrumentId) -> object:
    """
    Quote.
    """
    return QuoteTick(
        instrument_id=audusd_id,
        bid_price=Price.from_str("1.00000"),
        ask_price=Price.from_str("1.00001"),
        bid_size=Quantity.from_int(1),
        ask_size=Quantity.from_int(1),
        ts_event=3,
        ts_init=4,
    )


def test_quote_fully_qualified_name() -> None:
    """
    Test quote fully qualified name.
    """
    assert QuoteTick.fully_qualified_name() == "nautilus_trader.model:QuoteTick"
    assert QuoteTick.__module__ == "nautilus_trader.model"


def test_quote_construction(quote: object, audusd_id: InstrumentId) -> None:
    """
    Test quote construction.
    """
    assert quote.instrument_id == audusd_id
    assert quote.bid_price == Price.from_str("1.00000")
    assert quote.ask_price == Price.from_str("1.00001")
    assert quote.bid_size == Quantity.from_int(1)
    assert quote.ask_size == Quantity.from_int(1)
    assert quote.ts_event == 3
    assert quote.ts_init == 4


def test_quote_hash_str_and_repr(quote: object) -> None:
    """
    Test quote hash str and repr.
    """
    assert isinstance(hash(quote), int)
    assert str(quote) == "AUD/USD.SIM,1.00000,1.00001,1,1,3"
    assert repr(quote) == "QuoteTick(AUD/USD.SIM,1.00000,1.00001,1,1,3)"


def test_quote_equality(audusd_id: InstrumentId) -> None:
    """
    Test quote equality.
    """
    quote1 = QuoteTick(
        instrument_id=audusd_id,
        bid_price=Price.from_str("1.00000"),
        ask_price=Price.from_str("1.00001"),
        bid_size=Quantity.from_int(1),
        ask_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )
    quote2 = QuoteTick(
        instrument_id=audusd_id,
        bid_price=Price.from_str("1.00000"),
        ask_price=Price.from_str("1.00001"),
        bid_size=Quantity.from_int(1),
        ask_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )

    assert quote1 == quote2


def test_quote_pickle_roundtrip(quote: object) -> None:
    """
    Test quote pickle roundtrip.
    """
    pickled = pickle.dumps(quote)
    unpickled = pickle.loads(pickled)

    assert unpickled == quote
    assert unpickled.instrument_id == quote.instrument_id
    assert unpickled.bid_price == quote.bid_price


def test_quote_extract_price(audusd_id: InstrumentId) -> None:
    """
    Test quote extract price.
    """
    quote = QuoteTick(
        instrument_id=audusd_id,
        bid_price=Price.from_str("1.00000"),
        ask_price=Price.from_str("1.00001"),
        bid_size=Quantity.from_int(1),
        ask_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )

    assert quote.extract_price(PriceType.ASK) == Price.from_str("1.00001")
    assert quote.extract_price(PriceType.MID) == Price.from_str("1.000005")
    assert quote.extract_price(PriceType.BID) == Price.from_str("1.00000")


def test_quote_extract_size(audusd_id: InstrumentId) -> None:
    """
    Test quote extract size.
    """
    quote = QuoteTick(
        instrument_id=audusd_id,
        bid_price=Price.from_str("1.00000"),
        ask_price=Price.from_str("1.00001"),
        bid_size=Quantity.from_int(500_000),
        ask_size=Quantity.from_int(800_000),
        ts_event=0,
        ts_init=0,
    )

    assert quote.extract_size(PriceType.ASK) == Quantity.from_int(800_000)
    assert quote.extract_size(PriceType.MID) == Quantity.from_int(650_000)
    assert quote.extract_size(PriceType.BID) == Quantity.from_int(500_000)


def test_quote_extract_price_last_raises(audusd_id: InstrumentId) -> None:
    """
    Test quote extract price last raises.
    """
    quote = QuoteTick(
        instrument_id=audusd_id,
        bid_price=Price.from_str("1.00000"),
        ask_price=Price.from_str("1.00001"),
        bid_size=Quantity.from_int(1),
        ask_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )

    # A quote has no `Last` price: extraction raises a clean `ValueError` (not a panic)
    with pytest.raises(ValueError, match="price type LAST"):
        quote.extract_price(PriceType.LAST)


def test_quote_extract_size_last_raises(audusd_id: InstrumentId) -> None:
    """
    Test quote extract size last raises.
    """
    quote = QuoteTick(
        instrument_id=audusd_id,
        bid_price=Price.from_str("1.00000"),
        ask_price=Price.from_str("1.00001"),
        bid_size=Quantity.from_int(1),
        ask_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )

    with pytest.raises(ValueError, match="price type LAST"):
        quote.extract_size(PriceType.LAST)


def test_quote_to_dict(audusd_id: InstrumentId) -> None:
    """
    Test quote to dict.
    """
    quote = QuoteTick(
        instrument_id=audusd_id,
        bid_price=Price.from_str("1.00000"),
        ask_price=Price.from_str("1.00001"),
        bid_size=Quantity.from_int(1),
        ask_size=Quantity.from_int(1),
        ts_event=1,
        ts_init=2,
    )

    result = quote.to_dict()

    assert result == {
        "type": "QuoteTick",
        "instrument_id": "AUD/USD.SIM",
        "bid_price": "1.00000",
        "ask_price": "1.00001",
        "bid_size": "1",
        "ask_size": "1",
        "ts_event": 1,
        "ts_init": 2,
    }


def test_quote_from_dict_roundtrip(audusd_id: InstrumentId) -> None:
    """
    Test quote from dict roundtrip.
    """
    quote = QuoteTick(
        instrument_id=audusd_id,
        bid_price=Price.from_str("1.00000"),
        ask_price=Price.from_str("1.00001"),
        bid_size=Quantity.from_int(500_000),
        ask_size=Quantity.from_int(800_000),
        ts_event=1,
        ts_init=2,
    )

    restored = QuoteTick.from_dict(quote.to_dict())

    assert restored == quote


def test_quote_from_raw(audusd_id: InstrumentId) -> None:
    """
    Test quote from raw.
    """
    quote = QuoteTick.from_raw(
        instrument_id=audusd_id,
        bid_price_raw=10_000_000_000_000_000,
        ask_price_raw=10_000_100_000_000_000,
        bid_price_prec=5,
        ask_price_prec=5,
        bid_size_raw=5_000_000_000_000_000_000_000,
        ask_size_raw=8_000_000_000_000_000_000_000,
        bid_size_prec=0,
        ask_size_prec=0,
        ts_event=1,
        ts_init=2,
    )

    assert quote.instrument_id == audusd_id
    assert quote.bid_price == Price.from_str("1.00000")
    assert quote.ask_price == Price.from_str("1.00001")
    assert quote.ts_event == 1
    assert quote.ts_init == 2


def test_quote_from_raw_rejects_invalid_precision(audusd_id: InstrumentId) -> None:
    """
    Test quote from raw rejects invalid precision.
    """
    with pytest.raises(ValueError, match="exceeded maximum") as exc_info:
        QuoteTick.from_raw(
            instrument_id=audusd_id,
            bid_price_raw=10_000_000_000_000_000,
            ask_price_raw=10_000_100_000_000_000,
            bid_price_prec=5,
            ask_price_prec=255,
            bid_size_raw=5_000_000_000_000_000_000_000,
            ask_size_raw=8_000_000_000_000_000_000_000,
            bid_size_prec=0,
            ask_size_prec=0,
            ts_event=1,
            ts_init=2,
        )

    assert str(exc_info.value) == "`precision` exceeded maximum `WEI_PRECISION` (18), was 255"


def test_quote_from_raw_rejects_out_of_range_size(audusd_id: InstrumentId) -> None:
    """
    Test quote from raw rejects out of range size.
    """
    with pytest.raises(ValueError, match="exceeds QUANTITY_RAW_MAX") as exc_info:
        QuoteTick.from_raw(
            instrument_id=audusd_id,
            bid_price_raw=10_000_000_000_000_000,
            ask_price_raw=10_000_100_000_000_000,
            bid_price_prec=5,
            ask_price_prec=5,
            bid_size_raw=340_282_366_920_930_000_000_000_000_001,
            ask_size_raw=8_000_000_000_000_000_000_000,
            bid_size_prec=0,
            ask_size_prec=0,
            ts_event=1,
            ts_init=2,
        )

    assert str(exc_info.value) == (
        "raw value 340282366920930000000000000001 exceeds "
        "QUANTITY_RAW_MAX=340282366920930000000000000000"
    )


@pytest.mark.parametrize(
    ("index", "value", "message"),
    [
        (
            0,
            "AUDUSD",
            "invalid `InstrumentId` value 'AUDUSD': "
            "missing '.' separator between symbol and venue components",
        ),
        (
            2,
            -170_141_183_460_460_000_000_000_000_001,
            "raw value -170141183460460000000000000001 outside valid range "
            "[-170141183460460000000000000000, 170141183460460000000000000000]",
        ),
        (3, 255, "`precision` exceeded maximum `WEI_PRECISION` (18), was 255"),
        (
            6,
            340_282_366_920_930_000_000_000_000_001,
            "raw value 340282366920930000000000000001 exceeds "
            "QUANTITY_RAW_MAX=340282366920930000000000000000",
        ),
        (8, 19, "`precision` exceeded maximum `WEI_PRECISION` (18), was 19"),
    ],
)
def test_quote_setstate_rejects_invalid_state_without_mutation(
    quote: object,
    usdjpy_id: InstrumentId,
    index: int,
    value: object,
    message: str,
) -> None:
    """
    Test quote setstate rejects invalid state without mutation.
    """
    original_state = quote.__getstate__()
    other = QuoteTick(
        instrument_id=usdjpy_id,
        bid_price=Price.from_str("150.000"),
        ask_price=Price.from_str("150.001"),
        bid_size=Quantity.from_int(2),
        ask_size=Quantity.from_int(3),
        ts_event=5,
        ts_init=6,
    )
    state = list(other.__getstate__())
    state[index] = value

    with pytest.raises(ValueError, match=re.escape(message)) as exc_info:
        quote.__setstate__(tuple(state))

    assert str(exc_info.value) == message
    assert quote.__getstate__() == original_state


def test_quote_pickle_roundtrip_preserves_maximum_precision(audusd_id: InstrumentId) -> None:
    """
    Test quote pickle roundtrip preserves maximum precision.
    """
    quote = QuoteTick(
        instrument_id=audusd_id,
        bid_price=Price.from_str("1.000000000000000001"),
        ask_price=Price.from_str("1.000000000000000002"),
        bid_size=Quantity.from_str("0.000000000000000003"),
        ask_size=Quantity.from_str("0.000000000000000004"),
        ts_event=5,
        ts_init=6,
    )

    restored = pickle.loads(pickle.dumps(quote))

    assert restored.__getstate__() == quote.__getstate__()
    assert restored.bid_price.precision == 18
    assert restored.ask_size.raw == 4


@pytest.mark.parametrize(
    ("raw", "precision"),
    [(-(2**127), 4), (2**127 - 1, 0)],
)
def test_quote_pickle_roundtrip_preserves_sentinel_prices(
    audusd_id: InstrumentId,
    raw: int,
    precision: int,
) -> None:
    """
    Test quote pickle roundtrip preserves sentinel prices.
    """
    quote = QuoteTick.from_raw(
        instrument_id=audusd_id,
        bid_price_raw=raw,
        ask_price_raw=raw,
        bid_price_prec=precision,
        ask_price_prec=precision,
        bid_size_raw=10_000_000_000_000_000,
        ask_size_raw=20_000_000_000_000_000,
        bid_size_prec=0,
        ask_size_prec=0,
        ts_event=1,
        ts_init=2,
    )

    restored = pickle.loads(pickle.dumps(quote))

    assert restored.__getstate__() == quote.__getstate__()
    assert restored.bid_price.raw == raw
    assert restored.bid_price.precision == precision


def test_quote_pickle_roundtrip_preserves_mismatched_precisions() -> None:
    """
    Test quote pickle roundtrip preserves mismatched precisions.
    """
    quote = QuoteTick.from_dict(
        {
            "type": "QuoteTick",
            "instrument_id": "AUD/USD.SIM",
            "bid_price": "1.0",
            "ask_price": "1.00001",
            "bid_size": "1",
            "ask_size": "1.0",
            "ts_event": 1,
            "ts_init": 2,
        },
    )

    restored = pickle.loads(pickle.dumps(quote))

    assert restored.__getstate__() == quote.__getstate__()
    assert restored.bid_price.precision == 1
    assert restored.ask_price.precision == 5
