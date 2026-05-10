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

from nautilus_trader.model import AggressorSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import TradeId
from nautilus_trader.model import TradeTick


@pytest.fixture
def trade(audusd_id):
    return TradeTick(
        instrument_id=audusd_id,
        price=Price.from_str("1.00001"),
        size=Quantity.from_int(10_000),
        aggressor_side=AggressorSide.BUYER,
        trade_id=TradeId("123456"),
        ts_event=1,
        ts_init=2,
    )


def test_trade_fully_qualified_name():
    module_name, _, type_name = TradeTick.fully_qualified_name().partition(":")

    assert module_name
    assert type_name == "TradeTick"
    assert TradeTick.__module__ == "nautilus_trader.model"


def test_trade_construction(trade, audusd_id):
    assert trade.instrument_id == audusd_id
    assert trade.price == Price.from_str("1.00001")
    assert trade.size == Quantity.from_int(10_000)
    assert trade.aggressor_side == AggressorSide.BUYER
    assert trade.trade_id == TradeId("123456")
    assert trade.ts_event == 1
    assert trade.ts_init == 2


def test_trade_hash_str_and_repr(audusd_id):
    trade = TradeTick(
        instrument_id=audusd_id,
        price=Price.from_str("1.00000"),
        size=Quantity.from_int(50_000),
        aggressor_side=AggressorSide.BUYER,
        trade_id=TradeId("123456789"),
        ts_event=1,
        ts_init=2,
    )

    assert isinstance(hash(trade), int)
    assert str(trade) == "AUD/USD.SIM,1.00000,50000,BUYER,123456789,1"
    assert repr(trade) == "TradeTick(AUD/USD.SIM,1.00000,50000,BUYER,123456789,1)"


def test_trade_equality(audusd_id):
    trade1 = TradeTick(
        instrument_id=audusd_id,
        price=Price.from_str("1.00001"),
        size=Quantity.from_int(50_000),
        aggressor_side=AggressorSide.BUYER,
        trade_id=TradeId("123456"),
        ts_event=0,
        ts_init=0,
    )
    trade2 = TradeTick(
        instrument_id=audusd_id,
        price=Price.from_str("1.00001"),
        size=Quantity.from_int(50_000),
        aggressor_side=AggressorSide.BUYER,
        trade_id=TradeId("123456"),
        ts_event=0,
        ts_init=0,
    )

    assert trade1 == trade2


def test_trade_pickle_roundtrip(audusd_id):
    trade = TradeTick(
        instrument_id=audusd_id,
        price=Price.from_str("1.00001"),
        size=Quantity.from_int(50_000),
        aggressor_side=AggressorSide.SELLER,
        trade_id=TradeId("789"),
        ts_event=5,
        ts_init=6,
    )

    pickled = pickle.dumps(trade)
    unpickled = pickle.loads(pickled)  # noqa: S301

    assert unpickled == trade


def test_trade_to_dict(audusd_id):
    trade = TradeTick(
        instrument_id=audusd_id,
        price=Price.from_str("1.00000"),
        size=Quantity.from_int(10_000),
        aggressor_side=AggressorSide.BUYER,
        trade_id=TradeId("123456789"),
        ts_event=1,
        ts_init=2,
    )

    result = trade.to_dict()

    assert result == {
        "type": "TradeTick",
        "instrument_id": "AUD/USD.SIM",
        "price": "1.00000",
        "size": "10000",
        "aggressor_side": "BUYER",
        "trade_id": "123456789",
        "ts_event": 1,
        "ts_init": 2,
    }


def test_trade_from_dict_roundtrip(audusd_id):
    trade = TradeTick(
        instrument_id=audusd_id,
        price=Price.from_str("1.00001"),
        size=Quantity.from_int(50_000),
        aggressor_side=AggressorSide.BUYER,
        trade_id=TradeId("TRADE-1"),
        ts_event=100,
        ts_init=200,
    )

    restored = TradeTick.from_dict(trade.to_dict())

    assert restored == trade


def test_trade_from_raw(audusd_id):
    price = Price.from_str("1.00001")
    size = Quantity.from_int(10)

    trade = TradeTick.from_raw(
        instrument_id=audusd_id,
        price_raw=price.raw,
        price_prec=price.precision,
        size_raw=size.raw,
        size_prec=size.precision,
        aggressor_side=AggressorSide.BUYER,
        trade_id=TradeId("RAW-001"),
        ts_event=1,
        ts_init=2,
    )

    assert trade.instrument_id == audusd_id
    assert trade.price == price
    assert trade.size == size
    assert trade.aggressor_side == AggressorSide.BUYER
    assert trade.trade_id == TradeId("RAW-001")
    assert trade.ts_event == 1
    assert trade.ts_init == 2
