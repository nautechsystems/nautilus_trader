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

from nautilus_trader.core import UUID4
from nautilus_trader.model import AccountId
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import Currency
from nautilus_trader.model import LiquiditySide
from nautilus_trader.model import Money
from nautilus_trader.model import OrderFilled
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderType
from nautilus_trader.model import Position
from nautilus_trader.model import PositionId
from nautilus_trader.model import PositionSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TradeId
from nautilus_trader.model import TraderId
from nautilus_trader.model import VenueOrderId
from tests.providers import TestInstrumentProvider


USD = Currency.from_str("USD")
AUDUSD_SIM = TestInstrumentProvider.audusd_sim()


def _make_fill(
    instrument=None,
    order_side=OrderSide.BUY,
    last_px="1.00001",
    last_qty=100_000,
    position_id="P-123456",
    client_order_id="O-20210410-022422-001-001-1",
    venue_order_id="1",
    trade_id="E-20210410-022422-001-001-1",
    commission="2.00 USD",
    ts_event=0,
):
    if instrument is None:
        instrument = AUDUSD_SIM
    return OrderFilled(
        trader_id=TraderId("TESTER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId(client_order_id),
        venue_order_id=VenueOrderId(venue_order_id),
        account_id=AccountId("SIM-000"),
        trade_id=TradeId(trade_id),
        order_side=order_side,
        order_type=OrderType.MARKET,
        last_qty=Quantity.from_int(last_qty),
        last_px=Price.from_str(last_px),
        currency=instrument.quote_currency,
        liquidity_side=LiquiditySide.TAKER,
        event_id=UUID4(),
        ts_event=ts_event,
        ts_init=0,
        reconciliation=False,
        position_id=PositionId(position_id),
        commission=Money.from_str(commission),
    )


@pytest.fixture
def long_position():
    fill = _make_fill(order_side=OrderSide.BUY)
    return Position(instrument=AUDUSD_SIM, fill=fill)


@pytest.fixture
def short_position():
    fill = _make_fill(order_side=OrderSide.SELL)
    return Position(instrument=AUDUSD_SIM, fill=fill)


def test_position_long_properties(long_position):
    last = Price.from_str("1.00050")

    assert long_position.instrument_id == AUDUSD_SIM.id
    assert long_position.symbol == AUDUSD_SIM.id.symbol
    assert long_position.venue == AUDUSD_SIM.id.venue
    assert long_position.opening_order_id == ClientOrderId("O-20210410-022422-001-001-1")
    assert long_position.closing_order_id is None
    assert long_position.quantity == Quantity.from_int(100_000)
    assert long_position.peak_qty == Quantity.from_int(100_000)
    assert long_position.signed_qty == 100_000.0
    assert long_position.entry == OrderSide.BUY
    assert long_position.side == PositionSide.LONG
    assert long_position.ts_opened == 0
    assert long_position.duration_ns == 0
    assert long_position.avg_px_open == 1.00001
    assert long_position.event_count == 1
    assert long_position.id == PositionId("P-123456")
    assert long_position.is_long
    assert not long_position.is_short
    assert long_position.is_open
    assert not long_position.is_closed
    assert long_position.realized_return == 0
    assert long_position.realized_pnl == Money(-2.00, USD)
    assert long_position.unrealized_pnl(last) == Money(49.00, USD)
    assert long_position.total_pnl(last) == Money(47.00, USD)
    assert long_position.commissions() == [Money(2.00, USD)]


def test_position_short_properties(short_position):
    last = Price.from_str("1.00050")

    assert short_position.quantity == Quantity.from_int(100_000)
    assert short_position.signed_qty == -100_000.0
    assert short_position.side == PositionSide.SHORT
    assert short_position.avg_px_open == 1.00001
    assert short_position.event_count == 1
    assert short_position.id == PositionId("P-123456")
    assert not short_position.is_long
    assert short_position.is_short
    assert short_position.is_open
    assert not short_position.is_closed
    assert short_position.realized_return == 0
    assert short_position.realized_pnl == Money(-2.00, USD)
    assert short_position.unrealized_pnl(last) == Money(-49.00, USD)
    assert short_position.total_pnl(last) == Money(-51.00, USD)
    assert short_position.commissions() == [Money(2.00, USD)]


def test_position_str_and_repr(long_position):
    assert str(long_position) == "Position(LONG 100_000 AUD/USD.SIM, id=P-123456)"
    assert repr(long_position) == "Position(LONG 100_000 AUD/USD.SIM, id=P-123456)"


def test_position_equality(long_position):
    assert long_position == long_position


def test_position_events_and_ids(long_position):
    assert len(long_position.events()) == 1
    assert long_position.client_order_ids() == [ClientOrderId("O-20210410-022422-001-001-1")]
    assert long_position.venue_order_ids() == [VenueOrderId("1")]
    assert long_position.trade_ids() == [TradeId("E-20210410-022422-001-001-1")]
    assert long_position.last_trade_id == TradeId("E-20210410-022422-001-001-1")


def test_position_to_dict(long_position):
    d = long_position.to_dict()

    assert d["type"] == "Position"
    assert d["instrument_id"] == "AUD/USD.SIM"
    assert d["side"] == "LONG"
    assert d["entry"] == "BUY"
    assert d["quantity"] == "100000"
    assert d["avg_px_open"] == 1.00001
    assert d["realized_pnl"] == "-2.00 USD"


def test_position_partial_fill_long():
    fill = _make_fill(
        order_side=OrderSide.BUY,
        last_qty=50_000,
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill)
    last = Price.from_str("1.00048")

    assert position.quantity == Quantity.from_int(50_000)
    assert position.peak_qty == Quantity.from_int(50_000)
    assert position.side == PositionSide.LONG
    assert position.avg_px_open == 1.00001
    assert position.is_open
    assert position.unrealized_pnl(last) == Money(23.50, USD)
    assert repr(position) == "Position(LONG 50_000 AUD/USD.SIM, id=P-123456)"


def test_position_close_long():
    fill1 = _make_fill(
        order_side=OrderSide.BUY,
        last_px="1.00001",
        commission="3.00 USD",
        ts_event=1_000_000_000,
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill1)

    fill2 = _make_fill(
        order_side=OrderSide.SELL,
        last_px="1.00011",
        client_order_id="O-20210410-022422-001-001-2",
        venue_order_id="2",
        trade_id="E2",
        commission="0.00 USD",
        ts_event=2_000_000_000,
    )
    position.apply(fill2)
    last = Price.from_str("1.00050")

    assert position.quantity == Quantity.zero()
    assert position.side == PositionSide.FLAT
    assert position.ts_opened == 1_000_000_000
    assert position.duration_ns == 1_000_000_000
    assert position.avg_px_open == 1.00001
    assert position.avg_px_close == 1.00011
    assert position.ts_closed == 2_000_000_000
    assert position.event_count == 2
    assert not position.is_long
    assert not position.is_short
    assert not position.is_open
    assert position.is_closed
    assert position.realized_pnl == Money(7.00, USD)
    assert position.unrealized_pnl(last) == Money(0, USD)
    assert position.total_pnl(last) == Money(7.00, USD)
    assert repr(position) == "Position(FLAT AUD/USD.SIM, id=P-123456)"


def test_position_close_short():
    fill1 = _make_fill(
        order_side=OrderSide.SELL,
        last_px="1.00010",
        ts_event=1_000_000_000,
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill1)

    fill2 = _make_fill(
        order_side=OrderSide.BUY,
        last_px="1.00000",
        client_order_id="O-20210410-022422-001-001-2",
        venue_order_id="2",
        trade_id="E2",
        ts_event=2_000_000_000,
    )
    position.apply(fill2)

    assert position.side == PositionSide.FLAT
    assert position.is_closed
    assert position.avg_px_open == 1.00010
    assert position.avg_px_close == 1.00000


def test_position_partial_fills_then_close():
    fill1 = _make_fill(
        order_side=OrderSide.SELL,
        last_px="1.00001",
        last_qty=50_000,
        trade_id="E1",
    )
    fill2 = _make_fill(
        order_side=OrderSide.SELL,
        last_px="1.00002",
        last_qty=50_000,
        client_order_id="O-20210410-022422-001-001-2",
        venue_order_id="2",
        trade_id="E2",
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill1)
    position.apply(fill2)

    assert position.quantity == Quantity.from_int(100_000)
    assert position.side == PositionSide.SHORT
    assert position.avg_px_open == 1.000015
    assert position.event_count == 2
    assert position.commissions() == [Money(4.00, USD)]

    fill3 = _make_fill(
        order_side=OrderSide.BUY,
        last_px="1.00001",
        client_order_id="O-20210410-022422-001-001-3",
        venue_order_id="3",
        trade_id="E3",
    )
    position.apply(fill3)

    assert position.side == PositionSide.FLAT
    assert position.is_closed


def test_position_no_change():
    fill1 = _make_fill(
        order_side=OrderSide.BUY,
        last_px="1.0",
        last_qty=50_000,
        trade_id="E1",
    )
    fill2 = _make_fill(
        order_side=OrderSide.SELL,
        last_px="1.0",
        last_qty=50_000,
        client_order_id="O-20210410-022422-001-001-2",
        venue_order_id="2",
        trade_id="E2",
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill1)
    position.apply(fill2)
    last = Price.from_str("1.00050")

    assert position.side == PositionSide.FLAT
    assert position.is_closed
    assert position.avg_px_open == 1.0
    assert position.avg_px_close == 1.0
    assert position.realized_return == 0
    assert position.realized_pnl == Money(-4.00, USD)
    assert position.unrealized_pnl(last) == Money(0, USD)
    assert position.total_pnl(last) == Money(-4.00, USD)
    assert position.commissions() == [Money(4.00, USD)]


def test_position_multiple_fills_long():
    fill1 = _make_fill(
        order_side=OrderSide.BUY,
        last_px="1.00000",
        last_qty=50_000,
        trade_id="E1",
    )
    fill2 = _make_fill(
        order_side=OrderSide.BUY,
        last_px="1.00010",
        last_qty=50_000,
        client_order_id="O-20210410-022422-001-001-2",
        venue_order_id="2",
        trade_id="E2",
    )
    fill3 = _make_fill(
        order_side=OrderSide.SELL,
        last_px="1.00020",
        client_order_id="O-20210410-022422-001-001-3",
        venue_order_id="3",
        trade_id="E3",
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill1)
    position.apply(fill2)

    assert position.quantity == Quantity.from_int(100_000)
    assert position.avg_px_open == 1.00005
    assert position.is_long
    assert position.is_open

    position.apply(fill3)

    assert position.side == PositionSide.FLAT
    assert position.is_closed
    assert position.avg_px_close == 1.00020


def test_position_pnl_long_win():
    fill = _make_fill(
        order_side=OrderSide.BUY,
        last_px="1.00000",
        commission="0.00 USD",
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill)
    last = Price.from_str("1.00010")

    pnl = position.unrealized_pnl(last)

    assert pnl == Money(10.00, USD)


def test_position_pnl_long_loss():
    fill = _make_fill(
        order_side=OrderSide.BUY,
        last_px="1.00010",
        commission="0.00 USD",
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill)
    last = Price.from_str("1.00000")

    pnl = position.unrealized_pnl(last)

    assert pnl == Money(-10.00, USD)


def test_position_pnl_short_win():
    fill = _make_fill(
        order_side=OrderSide.SELL,
        last_px="1.00010",
        commission="0.00 USD",
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill)
    last = Price.from_str("1.00000")

    pnl = position.unrealized_pnl(last)

    assert pnl == Money(10.00, USD)


def test_position_pnl_short_loss():
    fill = _make_fill(
        order_side=OrderSide.SELL,
        last_px="1.00000",
        commission="0.00 USD",
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill)
    last = Price.from_str("1.00010")

    pnl = position.unrealized_pnl(last)

    assert pnl == Money(-10.00, USD)
