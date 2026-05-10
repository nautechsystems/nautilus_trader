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

import pickle

import pytest

from nautilus_trader.model import Price
from nautilus_trader.model import PriceType
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick


@pytest.fixture
def quote(audusd_id):
    return QuoteTick(
        instrument_id=audusd_id,
        bid_price=Price.from_str("1.00000"),
        ask_price=Price.from_str("1.00001"),
        bid_size=Quantity.from_int(1),
        ask_size=Quantity.from_int(1),
        ts_event=3,
        ts_init=4,
    )


def test_quote_fully_qualified_name():
    module_name, _, type_name = QuoteTick.fully_qualified_name().partition(":")

    assert module_name
    assert type_name == "QuoteTick"
    assert QuoteTick.__module__ == "nautilus_trader.model"


def test_quote_construction(quote, audusd_id):
    assert quote.instrument_id == audusd_id
    assert quote.bid_price == Price.from_str("1.00000")
    assert quote.ask_price == Price.from_str("1.00001")
    assert quote.bid_size == Quantity.from_int(1)
    assert quote.ask_size == Quantity.from_int(1)
    assert quote.ts_event == 3
    assert quote.ts_init == 4


def test_quote_hash_str_and_repr(quote):
    assert isinstance(hash(quote), int)
    assert str(quote) == "AUD/USD.SIM,1.00000,1.00001,1,1,3"
    assert repr(quote) == "QuoteTick(AUD/USD.SIM,1.00000,1.00001,1,1,3)"


def test_quote_equality(audusd_id):
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


def test_quote_pickle_roundtrip(quote):
    pickled = pickle.dumps(quote)
    unpickled = pickle.loads(pickled)  # noqa: S301

    assert unpickled == quote
    assert unpickled.instrument_id == quote.instrument_id
    assert unpickled.bid_price == quote.bid_price


def test_quote_extract_price(audusd_id):
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


def test_quote_extract_size(audusd_id):
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


def test_quote_to_dict(audusd_id):
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


def test_quote_from_dict_roundtrip(audusd_id):
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


def test_quote_from_raw(audusd_id):
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
