# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
from nautilus_trader.core.nautilus_pyo3 import Currency
from nautilus_trader.core.nautilus_pyo3 import LiquiditySide
from nautilus_trader.core.nautilus_pyo3 import Money
from nautilus_trader.core.nautilus_pyo3 import OrderFilled
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import OrderType
from nautilus_trader.core.nautilus_pyo3 import Position
from nautilus_trader.core.nautilus_pyo3 import PositionId
from nautilus_trader.core.nautilus_pyo3 import PositionSide
from nautilus_trader.core.nautilus_pyo3 import PositionSnapshot
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import StrategyId
from nautilus_trader.core.nautilus_pyo3 import TradeId
from nautilus_trader.core.nautilus_pyo3 import VenueOrderId
from nautilus_trader.test_kit.rust.accounting_pyo3 import TestAccountingProviderPyo3
from nautilus_trader.test_kit.rust.events_pyo3 import TestEventsProviderPyo3
from nautilus_trader.test_kit.rust.identifiers_pyo3 import TestIdProviderPyo3
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3
from nautilus_trader.test_kit.rust.orders_pyo3 import TestOrderProviderPyo3


AUDUSD_SIM = TestInstrumentProviderPyo3.default_fx_ccy("AUD/USD")
USD = Currency.from_str("USD")
USDT = Currency.from_str("USDT")
BTC = Currency.from_str("BTC")
ETH = Currency.from_str("ETH")


def test_position_hash_str_repr():
    # Arrange
    order = TestOrderProviderPyo3.market_order(
        instrument_id=AUDUSD_SIM.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
    )

    fill = TestEventsProviderPyo3.order_filled(
        order=order,
        instrument=AUDUSD_SIM,
        position_id=PositionId("P-123456"),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_str("1.00001"),
    )

    position = Position(instrument=AUDUSD_SIM, fill=fill)

    # Act, Assert
    assert str(position) == "Position(LONG 100_000 AUD/USD.SIM, id=P-123456)"
    assert repr(position) == "Position(LONG 100_000 AUD/USD.SIM, id=P-123456)"


def test_position_snapshot():
    # Arrange
    order = TestOrderProviderPyo3.market_order(
        instrument_id=AUDUSD_SIM.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
    )

    fill = TestEventsProviderPyo3.order_filled(
        order=order,
        instrument=AUDUSD_SIM,
        position_id=PositionId("P-123456"),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_str("1.00001"),
    )

    position = Position(instrument=AUDUSD_SIM, fill=fill)

    # Act
    values = position.to_dict()
    snapshot = PositionSnapshot.from_dict(values)

    # Assert
    # TODO: Assert all attributes
    assert snapshot


def test_position_to_from_dict():
    long_position = TestAccountingProviderPyo3.long_position()
    result_dict = long_position.to_dict()
    # Temporary for development and marked for removal
    # assert Position.from_dict(result_dict) == long_position
    assert result_dict == {
        "type": "Position",
        "account_id": "SIM-000",
        "avg_px_close": None,
        "avg_px_open": 1.00001,
        "base_currency": "AUD",
        "buy_qty": "100000",
        "closing_order_id": None,
        "commissions": ["2.00 USD"],
        "duration_ns": 0,
        "entry": "BUY",
        "events": [
            {
                "type": "OrderFilled",
                "account_id": "SIM-000",
                "client_order_id": "O-20210410-022422-001-001-1",
                "commission": "2.00 USD",
                "currency": "USD",
                "event_id": "2d89666b-1a1e-4a75-b193-4eb3b454c758",
                "info": {},
                "instrument_id": "AUD/USD.SIM",
                "last_px": "1.00001",
                "last_qty": "100000",
                "liquidity_side": "TAKER",
                "order_side": "BUY",
                "order_type": "MARKET",
                "position_id": "P-123456",
                "reconciliation": False,
                "strategy_id": "S-001",
                "trade_id": "E-20210410-022422-001-001-1",
                "trader_id": "TESTER-001",
                "ts_event": 0,
                "ts_init": 0,
                "venue_order_id": "1",
            },
        ],
        "position_id": "P-123456",
        "instrument_id": "AUD/USD.SIM",
        "is_inverse": False,
        "multiplier": "1",
        "opening_order_id": "O-20210410-022422-001-001-1",
        "peak_qty": "100000",
        "price_precision": 5,
        "quantity": "100000",
        "quote_currency": "USD",
        "realized_pnl": "-2.00 USD",
        "realized_return": 0.0,
        "sell_qty": "0",
        "settlement_currency": "USD",
        "side": "LONG",
        "signed_qty": 100000.0,
        "size_precision": 0,
        "strategy_id": "S-001",
        "trade_ids": ["E-20210410-022422-001-001-1"],
        "trader_id": "TESTER-001",
        "ts_closed": None,
        "ts_init": 0,
        "ts_last": 0,
        "ts_opened": 0,
        "venue_order_ids": ["1"],
    }


def test_position_filled_with_buy_order():
    order = TestOrderProviderPyo3.market_order(
        instrument_id=TestIdProviderPyo3.audusd_id(),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
    )
    position = TestAccountingProviderPyo3.long_position()

    last = Price.from_str("1.00050")
    assert position.symbol == AUDUSD_SIM.id.symbol
    assert position.venue == AUDUSD_SIM.id.venue
    assert position == position  # Equality operator test
    assert position.opening_order_id == ClientOrderId("O-20210410-022422-001-001-1")
    assert position.closing_order_id is None
    assert position.quantity == Quantity.from_int(100_000)
    assert position.peak_qty == Quantity.from_int(100_000)
    assert position.signed_qty == 100000
    assert position.entry == OrderSide.BUY
    assert position.side == PositionSide.LONG
    assert position.ts_opened == 0
    assert position.duration_ns == 0
    assert position.avg_px_open == 1.00001
    assert position.event_count == 1
    assert position.client_order_ids == [order.client_order_id]
    assert position.venue_order_ids == [VenueOrderId("1")]
    assert position.trade_ids == [TradeId("E-20210410-022422-001-001-1")]
    assert position.last_trade_id == TradeId("E-20210410-022422-001-001-1")
    assert position.id == PositionId("P-123456")
    assert len(position.events) == 1
    assert position.is_long
    assert not position.is_short
    assert not position.is_closed
    assert position.is_open
    assert position.realized_return == 0
    assert position.realized_pnl == Money(-2.00, USD)
    assert position.unrealized_pnl(last) == Money(49.00, USD)
    assert position.total_pnl(last) == Money(47.00, USD)
    assert position.commissions() == [Money(2.00, USD)]
    assert repr(position) == "Position(LONG 100_000 AUD/USD.SIM, id=P-123456)"


def test_position_filled_with_sell_order():
    position = TestAccountingProviderPyo3.short_position()
    last = Price.from_str("1.00050")

    assert position.quantity == Quantity.from_int(100_000)
    assert position.peak_qty == Quantity.from_int(100_000)
    assert position.size_precision == 0
    assert position.signed_qty == -100_000.0
    assert position.side == PositionSide.SHORT
    assert position.ts_opened == 0
    assert position.avg_px_open == 1.00001
    assert position.event_count == 1
    assert position.trade_ids == [TradeId("E-20210410-022422-001-001-1")]
    assert position.last_trade_id == TradeId("E-20210410-022422-001-001-1")
    assert position.id == PositionId("P-123456")
    assert not position.is_long
    assert position.is_short
    assert position.is_open
    assert not position.is_closed
    assert position.realized_return == 0
    assert position.realized_pnl == Money(-2.00, USD)
    assert position.unrealized_pnl(last) == Money(-49.00, USD)
    assert position.total_pnl(last) == Money(-51.00, USD)
    assert position.commissions() == [Money(2.00, USD)]
    assert repr(position) == "Position(SHORT 100_000 AUD/USD.SIM, id=P-123456)"


def test_position_partial_fills_with_buy_order() -> None:
    order = TestOrderProviderPyo3.market_order(
        instrument_id=TestIdProviderPyo3.audusd_id(),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
    )
    instrument = TestInstrumentProviderPyo3.audusd_sim()
    order_filled = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order,
        position_id=TestIdProviderPyo3.position_id(),
        last_px=Price.from_str("1.00001"),
        last_qty=Quantity.from_int(50_000),
    )
    position = Position(instrument=instrument, fill=order_filled)
    last = Price.from_str("1.00048")

    assert position.quantity == Quantity.from_int(50_000)
    assert position.peak_qty == Quantity.from_int(50_000)
    assert position.side == PositionSide.LONG
    assert position.ts_opened == 0
    assert position.avg_px_open == 1.00001
    assert position.event_count == 1
    assert position.is_long
    assert not position.is_short
    assert position.is_open
    assert not position.is_closed
    assert position.realized_return == 0
    assert position.realized_pnl == Money(-2.00, USD)
    assert position.unrealized_pnl(last) == Money(23.50, USD)
    assert position.total_pnl(last) == Money(21.50, USD)
    assert position.commissions() == [Money(2.00, USD)]
    assert repr(position) == "Position(LONG 50_000 AUD/USD.SIM, id=P-123456)"


def test_position_partial_fills_with_sell_order() -> None:
    order = TestOrderProviderPyo3.market_order(
        instrument_id=TestIdProviderPyo3.audusd_id(),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
    )
    instrument = TestInstrumentProviderPyo3.audusd_sim()
    fill1 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order,
        position_id=TestIdProviderPyo3.position_id(),
        last_px=Price.from_str("1.00001"),
        last_qty=Quantity.from_int(50_000),
        trade_id=TradeId("1"),
    )
    fill2 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order,
        position_id=TestIdProviderPyo3.position_id(),
        last_px=Price.from_str("1.00002"),
        last_qty=Quantity.from_int(50_000),
        trade_id=TradeId("2"),
    )
    position = Position(instrument=instrument, fill=fill1)
    last = Price.from_str("1.00050")

    position.apply(fill2)

    assert position.quantity == Quantity.from_int(100_000)
    assert position.side == PositionSide.SHORT
    assert position.ts_opened == 0
    assert position.avg_px_open == 1.000015
    assert position.event_count == 2
    assert not position.is_long
    assert position.is_short
    assert position.is_open
    assert not position.is_closed
    assert position.realized_return == 0
    assert position.realized_pnl == Money(-4.00, USD)
    assert position.unrealized_pnl(last) == Money(-48.50, USD)
    assert position.total_pnl(last) == Money(-52.50, USD)
    assert position.commissions() == [Money(4.00, USD)]
    assert repr(position) == "Position(SHORT 100_000 AUD/USD.SIM, id=P-123456)"


def test_position_filled_with_buy_order_then_sell_order():
    order = TestOrderProviderPyo3.market_order(
        instrument_id=TestIdProviderPyo3.audusd_id(),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(150_000),
    )
    instrument = TestInstrumentProviderPyo3.audusd_sim()
    fill1 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order,
        position_id=TestIdProviderPyo3.position_id(),
        last_px=Price.from_str("1.00001"),
        ts_filled_ns=1_000_000_000,
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill1)

    fill2 = OrderFilled(
        trader_id=TestIdProviderPyo3.trader_id(),
        strategy_id=StrategyId("S-001"),
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("2"),
        account_id=TestIdProviderPyo3.account_id(),
        trade_id=TradeId("E2"),
        position_id=PositionId("T123456"),
        order_side=OrderSide.SELL,
        order_type=OrderType.MARKET,
        last_qty=order.quantity,
        last_px=Price.from_str("1.00011"),
        currency=AUDUSD_SIM.quote_currency,
        commission=Money(0, USD),
        liquidity_side=LiquiditySide.TAKER,
        event_id=TestIdProviderPyo3.uuid(),
        ts_event=2_000_000_000,
        ts_init=0,
        reconciliation=False,
    )
    last = Price.from_str("1.00050")
    position.apply(fill2)

    assert position.is_opposite_side(fill2.order_side)
    assert position.quantity == Quantity.zero()
    assert position.size_precision == 0
    assert position.signed_qty == 0.0
    assert position.side == PositionSide.FLAT
    assert position.ts_opened == 1_000_000_000
    assert position.duration_ns == 1_000_000_000
    assert position.avg_px_open == 1.00001
    assert position.event_count == 2
    assert position.ts_closed == 2_000_000_000
    assert position.avg_px_close == 1.00011
    assert not position.is_long
    assert not position.is_short
    assert not position.is_open
    assert position.is_closed
    assert position.realized_return == 9.999900000998888e-05
    assert position.realized_pnl == Money(12.00, USD)
    assert position.unrealized_pnl(last) == Money(0, USD)
    assert position.total_pnl(last) == Money(12.00, USD)
    assert position.commissions() == [Money(3.00, USD)]
    assert repr(position) == "Position(FLAT AUD/USD.SIM, id=P-123456)"


def test_position_filled_with_sell_order_then_buy_order():
    order1 = TestOrderProviderPyo3.market_order(
        instrument_id=TestIdProviderPyo3.audusd_id(),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
        client_order_id=ClientOrderId("1"),
    )
    order2 = TestOrderProviderPyo3.market_order(
        instrument_id=TestIdProviderPyo3.audusd_id(),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        client_order_id=ClientOrderId("2"),
    )
    instrument = TestInstrumentProviderPyo3.audusd_sim()
    fill1 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order1,
        position_id=PositionId("P-19700101-000000-000-001-1"),
        trade_id=TradeId("1"),
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill1)
    fill2 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order2,
        position_id=TestIdProviderPyo3.position_id(),
        last_px=Price.from_str("1.00001"),
        last_qty=Quantity.from_int(50_000),
        trade_id=TradeId("2"),
    )
    fill3 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order2,
        position_id=TestIdProviderPyo3.position_id(),
        last_px=Price.from_str("1.00003"),
        last_qty=Quantity.from_int(50_000),
        trade_id=TradeId("3"),
    )
    last = Price.from_str("1.00050")
    position.apply(fill2)
    position.apply(fill3)

    # Assert
    assert position.quantity == Quantity.zero()
    assert position.side == PositionSide.FLAT
    assert position.ts_opened == 0
    assert position.avg_px_open == 1.0
    assert position.event_count == 3
    assert position.client_order_ids == [order1.client_order_id, order2.client_order_id]
    assert position.ts_closed == 0
    assert position.avg_px_close == 1.00002
    assert not position.is_long
    assert not position.is_short
    assert not position.is_open
    assert position.is_closed
    assert position.realized_pnl == Money(-8.00, USD)
    assert position.unrealized_pnl(last) == Money(0, USD)
    assert position.total_pnl(last) == Money(-8.000, USD)
    assert position.commissions() == [Money(6.00, USD)]
    assert repr(position) == "Position(FLAT AUD/USD.SIM, id=P-19700101-000000-000-001-1)"


def test_position_filled_with_no_change():
    order1 = TestOrderProviderPyo3.market_order(
        instrument_id=TestIdProviderPyo3.audusd_id(),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        client_order_id=ClientOrderId("1"),
    )
    order2 = TestOrderProviderPyo3.market_order(
        instrument_id=TestIdProviderPyo3.audusd_id(),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
        client_order_id=ClientOrderId("2"),
    )
    instrument = TestInstrumentProviderPyo3.audusd_sim()
    fill1 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order1,
        position_id=PositionId("P-19700101-000000-000-001-1"),
        last_px=Price.from_str("1.0"),
        last_qty=Quantity.from_int(50_000),
        trade_id=TradeId("E-19700101-000000-000-001-1"),
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill1)
    fill2 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order2,
        position_id=TestIdProviderPyo3.position_id(),
        last_px=Price.from_str("1.0"),
        last_qty=Quantity.from_int(50_000),
        trade_id=TradeId("E-19700101-000000-000-001-2"),
    )
    position.apply(fill2)
    last = Price.from_str("1.00050")

    assert position.quantity == Quantity.zero()
    assert position.side == PositionSide.FLAT
    assert position.ts_opened == 0
    assert position.avg_px_open == 1.0
    assert position.event_count == 2
    assert position.client_order_ids == [order1.client_order_id, order2.client_order_id]
    assert position.trade_ids == [
        TradeId("E-19700101-000000-000-001-1"),
        TradeId("E-19700101-000000-000-001-2"),
    ]
    assert position.ts_closed == 0
    assert position.avg_px_close == 1.0
    assert not position.is_long
    assert not position.is_short
    assert not position.is_open
    assert position.is_closed
    assert position.realized_return == 0
    assert position.realized_pnl == Money(-4.00, USD)
    assert position.unrealized_pnl(last) == Money(0, USD)
    assert position.total_pnl(last) == Money(-4.00, USD)
    assert position.commissions() == [Money(4.00, USD)]
    assert repr(position) == "Position(FLAT AUD/USD.SIM, id=P-19700101-000000-000-001-1)"


def test_position_long_with_multiple_filled_orders():
    order1 = TestOrderProviderPyo3.market_order(
        instrument_id=TestIdProviderPyo3.audusd_id(),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        client_order_id=ClientOrderId("1"),
    )
    order2 = TestOrderProviderPyo3.market_order(
        instrument_id=TestIdProviderPyo3.audusd_id(),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        client_order_id=ClientOrderId("2"),
    )
    order3 = TestOrderProviderPyo3.market_order(
        instrument_id=TestIdProviderPyo3.audusd_id(),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(200_000),
        client_order_id=ClientOrderId("3"),
    )
    instrument = TestInstrumentProviderPyo3.audusd_sim()
    fill1 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order1,
        position_id=TestIdProviderPyo3.position_id(),
        strategy_id=StrategyId("S-001"),
        trade_id=TradeId("1"),
    )
    fill2 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order2,
        position_id=TestIdProviderPyo3.position_id(),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_str("1.00001"),
        trade_id=TradeId("2"),
    )
    fill3 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order3,
        position_id=TestIdProviderPyo3.position_id(),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_str("1.00010"),
        trade_id=TradeId("3"),
    )
    last = Price.from_str("1.00050")

    position = Position(instrument=AUDUSD_SIM, fill=fill1)
    position.apply(fill2)
    position.apply(fill3)

    # Assert
    assert position.quantity == Quantity.zero()
    assert position.side == PositionSide.FLAT
    assert position.ts_opened == 0
    assert position.avg_px_open == 1.000005
    assert position.event_count == 3
    assert position.client_order_ids == [
        order1.client_order_id,
        order2.client_order_id,
        order3.client_order_id,
    ]
    assert position.ts_closed == 0
    assert position.avg_px_close == 1.0001
    assert not position.is_long
    assert not position.is_short
    assert not position.is_open
    assert position.is_closed
    assert position.realized_pnl == Money(11.00, USD)
    assert position.unrealized_pnl(last) == Money(0, USD)
    assert position.total_pnl(last) == Money(11.00, USD)
    assert position.commissions() == [Money(8.00, USD)]
    assert repr(position) == "Position(FLAT AUD/USD.SIM, id=P-123456)"


def test_pnl_calculation_from_trading_technologies_example():
    instrument = TestInstrumentProviderPyo3.ethusdt_binance()
    order1 = TestOrderProviderPyo3.market_order(
        instrument_id=instrument.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(12),
    )
    fill1 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order1,
        position_id=TestIdProviderPyo3.position_id(),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_int(100),
        trade_id=TradeId("1"),
    )
    position = Position(instrument=instrument, fill=fill1)
    order2 = TestOrderProviderPyo3.market_order(
        instrument_id=instrument.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(17),
    )
    fill2 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order2,
        position_id=TestIdProviderPyo3.position_id(),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_int(99),
        trade_id=TradeId("2"),
    )
    position.apply(fill2)
    assert position.quantity == Quantity.from_int(29)
    assert position.realized_pnl == Money(-0.28830000, USDT)
    assert position.avg_px_open == 99.41379310344827
    order3 = TestOrderProviderPyo3.market_order(
        instrument_id=instrument.id,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(9),
    )
    fill3 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order3,
        position_id=TestIdProviderPyo3.position_id(),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_int(101),
        trade_id=TradeId("3"),
    )
    position.apply(fill3)
    assert position.quantity == Quantity.from_int(20)
    assert position.realized_pnl == Money(13.89666207, USDT)
    assert position.avg_px_open == 99.41379310344827
    order4 = TestOrderProviderPyo3.market_order(
        instrument_id=instrument.id,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(4),
    )
    fill4 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order4,
        position_id=TestIdProviderPyo3.position_id(),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_int(105),
        trade_id=TradeId("4"),
    )
    position.apply(fill4)
    assert position.quantity == Quantity.from_int(16)
    assert position.realized_pnl == Money(36.19948966, USDT)
    assert position.avg_px_open == 99.41379310344827
    order5 = TestOrderProviderPyo3.market_order(
        instrument_id=instrument.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(3),
    )
    fill5 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order5,
        position_id=TestIdProviderPyo3.position_id(),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_int(103),
        trade_id=TradeId("5"),
    )
    position.apply(fill5)
    assert position.quantity == Quantity.from_int(19)
    assert position.realized_pnl == Money(36.16858966, USDT)
    assert position.avg_px_open == 99.98003629764065
    assert repr(position) == "Position(LONG 19.00000 ETHUSDT.BINANCE, id=P-123456)"


def test_position_closed_and_reopened() -> None:
    order = TestOrderProviderPyo3.market_order(
        instrument_id=TestIdProviderPyo3.audusd_id(),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(150_000),
    )
    fill1 = TestEventsProviderPyo3.order_filled(
        instrument=AUDUSD_SIM,
        order=order,
        position_id=TestIdProviderPyo3.position_id(),
        last_px=Price.from_str("1.00001"),
        ts_filled_ns=1_000_000_000,
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill1)
    fill2 = OrderFilled(
        trader_id=order.trader_id,
        strategy_id=StrategyId("S-001"),
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("2"),
        account_id=TestIdProviderPyo3.account_id(),
        trade_id=TradeId("2"),
        position_id=PositionId("P-123456"),
        order_side=OrderSide.SELL,
        order_type=OrderType.MARKET,
        last_qty=order.quantity,
        last_px=Price.from_str("1.00011"),
        currency=AUDUSD_SIM.quote_currency,
        commission=Money(0, USD),
        liquidity_side=LiquiditySide.TAKER,
        event_id=TestIdProviderPyo3.uuid(),
        ts_event=2_000_000_000,
        ts_init=0,
        reconciliation=False,
    )

    position.apply(fill2)

    fill3 = OrderFilled(
        trader_id=order.trader_id,
        strategy_id=StrategyId("S-001"),
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("2"),
        account_id=TestIdProviderPyo3.account_id(),
        trade_id=TradeId("3"),
        position_id=PositionId("P-123456"),
        order_side=OrderSide.BUY,
        order_type=OrderType.MARKET,
        last_qty=order.quantity,
        last_px=Price.from_str("1.00012"),
        currency=AUDUSD_SIM.quote_currency,
        commission=Money(0, USD),
        liquidity_side=LiquiditySide.TAKER,
        event_id=TestIdProviderPyo3.uuid(),
        ts_event=3_000_000_000,
        ts_init=0,
        reconciliation=False,
    )

    # Act
    position.apply(fill3)

    # Assert
    last = Price.from_str("1.00030")
    assert position.is_opposite_side(fill2.order_side)
    assert position.quantity == Quantity.from_int(150_000)
    assert position.peak_qty == Quantity.from_int(150_000)
    assert position.side == PositionSide.LONG
    assert position.opening_order_id == fill3.client_order_id
    assert position.closing_order_id is None
    assert position.ts_opened == 3_000_000_000
    assert position.duration_ns == 0
    assert position.avg_px_open == 1.00012
    assert position.event_count == 1
    assert position.ts_closed is None
    assert position.avg_px_close is None
    assert position.is_long
    assert position.is_open
    assert not position.is_short
    assert not position.is_closed
    assert position.realized_return == 0.0
    assert position.realized_pnl == Money(0.00, USD)
    assert position.unrealized_pnl(last) == Money(27.00, USD)
    assert position.total_pnl(last) == Money(27.00, USD)
    assert position.commissions() == [Money(0.00, USD)]
    assert repr(position) == "Position(LONG 150_000 AUD/USD.SIM, id=P-123456)"


def test_position_realized_pnl_with_interleaved_order_sides():
    instrument = TestInstrumentProviderPyo3.btcusdt_binance()
    order1 = TestOrderProviderPyo3.market_order(
        instrument_id=instrument.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(12),
    )
    fill1 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order1,
        position_id=TestIdProviderPyo3.position_id(),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_int(10000),
        trade_id=TradeId("1"),
    )
    position = Position(instrument=instrument, fill=fill1)
    order2 = TestOrderProviderPyo3.market_order(
        instrument_id=instrument.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(17),
    )
    fill2 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order2,
        position_id=TestIdProviderPyo3.position_id(),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_int(9999),
        trade_id=TradeId("2"),
    )
    position.apply(fill2)
    assert position.quantity == Quantity.from_str("29.000000")
    assert position.realized_pnl == Money(-289.98300000, USDT)
    assert position.avg_px_open == 9999.413793103447
    order3 = TestOrderProviderPyo3.market_order(
        instrument_id=instrument.id,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(9),
    )
    fill3 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order3,
        position_id=TestIdProviderPyo3.position_id(),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_int(10001),
        trade_id=TradeId("3"),
    )
    position.apply(fill3)
    assert position.quantity == Quantity.from_int(20)
    assert position.realized_pnl == Money(-365.71613793, USDT)
    assert position.avg_px_open == 9999.413793103447
    order4 = TestOrderProviderPyo3.market_order(
        instrument_id=instrument.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(3),
    )
    fill4 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order4,
        position_id=TestIdProviderPyo3.position_id(),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_int(10003),
        trade_id=TradeId("4"),
    )
    position.apply(fill4)
    assert position.quantity == Quantity.from_int(23)
    assert position.realized_pnl == Money(-395.72513793, USDT)
    assert position.avg_px_open == 9999.88155922039

    order5 = TestOrderProviderPyo3.market_order(
        instrument_id=instrument.id,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(4),
    )
    fill5 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order5,
        position_id=TestIdProviderPyo3.position_id(),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_int(10005),
        trade_id=TradeId("5"),
    )
    position.apply(fill5)
    assert position.quantity == Quantity.from_int(19)
    assert position.realized_pnl == Money(-415.27137481, USDT)
    assert position.avg_px_open == 9999.88155922039
    assert repr(position) == "Position(LONG 19.000000 BTCUSDT.BINANCE, id=P-123456)"


def test_calculate_pnl_when_given_position_side_flat_returns_zero():
    instrument = TestInstrumentProviderPyo3.btcusdt_binance()
    order = TestOrderProviderPyo3.market_order(
        instrument_id=instrument.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(12),
    )
    fill = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order,
        position_id=TestIdProviderPyo3.position_id(),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_int(10500),
        trade_id=TradeId("1"),
    )
    position = Position(instrument=instrument, fill=fill)
    result = position.calculate_pnl(10500.0, 10500.0, Quantity.from_int(100_000))
    assert result == Money(0, USDT)


def test_calculate_pnl_for_long_position_win() -> None:
    instrument = TestInstrumentProviderPyo3.btcusdt_binance()
    order = TestOrderProviderPyo3.market_order(
        instrument_id=instrument.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(12),
    )
    fill = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order,
        position_id=TestIdProviderPyo3.position_id(),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_int(10500),
        trade_id=TradeId("1"),
    )
    position = Position(instrument=instrument, fill=fill)
    pnl = position.calculate_pnl(
        avg_px_open=10500.00,
        avg_px_close=10510.00,
        quantity=Quantity.from_int(12),
    )

    # Assert
    assert pnl == Money(120.00000000, USDT)
    assert position.realized_pnl == Money(-126.00000000, USDT)
    assert position.unrealized_pnl(Price.from_str("10510.00")) == Money(120.00000000, USDT)
    assert position.total_pnl(Price.from_str("10510.00")) == Money(-6.00000000, USDT)
    assert position.commissions() == [Money(126.00000000, USDT)]


def test_calculate_pnl_for_long_position_loss() -> None:
    instrument = TestInstrumentProviderPyo3.btcusdt_binance()
    order = TestOrderProviderPyo3.market_order(
        instrument_id=TestIdProviderPyo3.audusd_id(),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(12),
    )
    fill = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order,
        position_id=TestIdProviderPyo3.position_id(),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_int(10500),
        trade_id=TradeId("1"),
    )
    position = Position(instrument=instrument, fill=fill)
    pnl = position.calculate_pnl(
        avg_px_open=10500.00,
        avg_px_close=10480.50,
        quantity=Quantity.from_int(10),
    )

    # Assert
    assert pnl == Money(-195.00000000, USDT)
    assert position.realized_pnl == Money(-126.00000000, USDT)
    assert position.unrealized_pnl(Price.from_str("10480.50")) == Money(-234.00000000, USDT)
    assert position.total_pnl(Price.from_str("10480.50")) == Money(-360.00000000, USDT)
    assert position.commissions() == [Money(126.00000000, USDT)]


def test_calculate_pnl_for_short_position_winning() -> None:
    instrument = TestInstrumentProviderPyo3.btcusdt_binance()
    order = TestOrderProviderPyo3.market_order(
        instrument_id=instrument.id,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_str("10.15"),
    )
    fill = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order,
        position_id=TestIdProviderPyo3.position_id(),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_int(10500),
        trade_id=TradeId("1"),
    )
    position = Position(instrument=instrument, fill=fill)

    pnl = position.calculate_pnl(
        10500.00,
        10390.00,
        Quantity.from_str("10.150000"),
    )

    assert pnl == Money(1116.50000000, USDT)
    assert position.unrealized_pnl(Price.from_str("10390.00")) == Money(1116.50000000, USDT)
    assert position.realized_pnl == Money(-106.57500000, USDT)
    assert position.commissions() == [Money(106.57500000, USDT)]
    assert position.notional_value(Price.from_str("10390.00")) == Money(105458.50000000, USDT)


def test_calculate_pnl_for_short_position_loss() -> None:
    instrument = TestInstrumentProviderPyo3.btcusdt_binance()
    order = TestOrderProviderPyo3.market_order(
        instrument_id=instrument.id,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_str("10"),
    )
    fill = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order,
        position_id=TestIdProviderPyo3.position_id(),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_int(10500),
        trade_id=TradeId("1"),
    )
    position = Position(instrument=instrument, fill=fill)
    pnl = position.calculate_pnl(
        10500.00,
        10670.50,
        Quantity.from_str("10.000000"),
    )

    # Assert
    assert pnl == Money(-1705.00000000, USDT)
    assert position.unrealized_pnl(Price.from_str("10670.50")) == Money(-1705.00000000, USDT)
    assert position.realized_pnl == Money(-105.00000000, USDT)
    assert position.commissions() == [Money(105.00000000, USDT)]
    assert position.notional_value(Price.from_str("10670.50")) == Money(106705.00000000, USDT)


def test_calculate_pnl_for_inverse1() -> None:
    instrument = TestInstrumentProviderPyo3.xbtusd_bitmex()
    order = TestOrderProviderPyo3.market_order(
        instrument_id=instrument.id,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
    )
    fill = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order,
        position_id=TestIdProviderPyo3.position_id(),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_int(10000),
        trade_id=TradeId("1"),
    )
    position = Position(instrument=instrument, fill=fill)
    pnl = position.calculate_pnl(
        avg_px_open=10000.00,
        avg_px_close=11000.00,
        quantity=Quantity.from_int(100_000),
    )

    assert pnl == Money(-0.90909091, BTC)
    assert position.unrealized_pnl(Price.from_str("11000.00")) == Money(-0.90909091, BTC)
    assert position.realized_pnl == Money(-0.00750000, BTC)
    assert position.notional_value(Price.from_str("11000.00")) == Money(9.09090909, BTC)


def test_calculate_pnl_for_inverse2() -> None:
    instrument = TestInstrumentProviderPyo3.ethusd_bitmex()
    order = TestOrderProviderPyo3.market_order(
        instrument_id=instrument.id,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
    )
    fill = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order,
        position_id=TestIdProviderPyo3.position_id(),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_str("375.95"),
        trade_id=TradeId("1"),
    )
    position = Position(instrument=instrument, fill=fill)

    assert position.unrealized_pnl(Price.from_str("370.00")) == Money(4.27745208, ETH)
    assert position.notional_value(Price.from_str("370.00")) == Money(270.27027027, ETH)


def test_calculate_unrealized_pnl_for_long() -> None:
    instrument = TestInstrumentProviderPyo3.btcusdt_binance()
    order1 = TestOrderProviderPyo3.market_order(
        instrument_id=instrument.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(2),
    )
    order2 = TestOrderProviderPyo3.market_order(
        instrument_id=instrument.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(2),
    )
    fill1 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order1,
        position_id=TestIdProviderPyo3.position_id(),
        last_px=Price.from_int(10500),
        trade_id=TradeId("1"),
    )
    fill2 = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order2,
        position_id=TestIdProviderPyo3.position_id(),
        last_px=Price.from_int(10500),
        trade_id=TradeId("2"),
    )
    position = Position(instrument=instrument, fill=fill1)
    position.apply(fill2)
    pnl = position.unrealized_pnl(Price.from_str("11505.60"))

    # Assert
    assert pnl == Money(4022.40000000, USDT)
    assert position.realized_pnl == Money(-42.00000000, USDT)
    assert position.commissions() == [Money(42.00000000, USDT)]


def test_calculate_unrealized_pnl_for_short() -> None:
    instrument = TestInstrumentProviderPyo3.btcusdt_binance()
    order = TestOrderProviderPyo3.market_order(
        instrument_id=instrument.id,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_str("5.912"),
    )
    fill = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order,
        position_id=TestIdProviderPyo3.position_id(),
        last_px=Price.from_str("10505.60"),
        trade_id=TradeId("1"),
    )
    position = Position(instrument=instrument, fill=fill)
    pnl = position.unrealized_pnl(Price.from_str("10407.15"))

    assert pnl == Money(582.03640000, USDT)
    assert position.realized_pnl == Money(-62.10910720, USDT)
    assert position.commissions() == [Money(62.10910720, USDT)]


def test_calculate_unrealized_pnl_for_long_inverse() -> None:
    instrument = TestInstrumentProviderPyo3.xbtusd_bitmex()
    order = TestOrderProviderPyo3.market_order(
        instrument_id=instrument.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
    )
    fill = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order,
        position_id=TestIdProviderPyo3.position_id(),
        last_px=Price.from_str("10500.00"),
        trade_id=TradeId("1"),
    )
    position = Position(instrument=instrument, fill=fill)
    pnl = position.unrealized_pnl(Price.from_str("11505.60"))

    # Assert
    assert pnl == Money(0.83238969, BTC)
    assert position.realized_pnl == Money(-0.00714286, BTC)
    assert position.commissions() == [Money(0.00714286, BTC)]


def test_calculate_unrealized_pnl_for_short_inverse() -> None:
    instrument = TestInstrumentProviderPyo3.xbtusd_bitmex()
    order = TestOrderProviderPyo3.market_order(
        instrument_id=instrument.id,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(1_250_000),
    )
    fill = TestEventsProviderPyo3.order_filled(
        instrument=instrument,
        order=order,
        position_id=TestIdProviderPyo3.position_id(),
        last_px=Price.from_str("15500.00"),
        trade_id=TradeId("1"),
    )
    position = Position(instrument=instrument, fill=fill)
    pnl = position.unrealized_pnl(Price.from_str("12506.65"))

    # Assert
    assert pnl == Money(19.30166700, BTC)
    assert position.realized_pnl == Money(-0.06048387, BTC)
    assert position.commissions() == [Money(0.06048387, BTC)]


@pytest.mark.parametrize(
    ("order_side", "quantity", "expected_signed_qty"),
    [
        [OrderSide.BUY, 25, 25.0],
        [OrderSide.SELL, 25, -25.0],
    ],
)
def test_signed_qty_decimal_qty_for_equity(
    order_side: OrderSide,
    quantity: int,
    expected_signed_qty: float,
) -> None:
    instrument = TestInstrumentProviderPyo3.aapl_equity()
    order = TestOrderProviderPyo3.market_order(
        instrument.id,
        order_side,
        Quantity.from_int(quantity),
    )

    fill = TestEventsProviderPyo3.order_filled(
        order,
        instrument=instrument,
        position_id=PositionId("P-123456"),
        strategy_id=StrategyId("S-001"),
        last_px=Price.from_str("100"),
    )

    # Act
    position = Position(instrument=instrument, fill=fill)

    # Assert
    assert position.signed_qty == expected_signed_qty
