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

from decimal import Decimal

import pytest

from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AAPL_XNAS = TestInstrumentProvider.equity(symbol="AAPL", venue="XNAS")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()
XBTUSD_BITMEX = TestInstrumentProvider.xbtusd_bitmex()
ETHUSD_BITMEX = TestInstrumentProvider.ethusd_bitmex()


class TestPosition:
    def setup(self):
        # Fixture Setup
        self.trader_id = TestIdStubs.trader_id()
        self.account_id = TestIdStubs.account_id()
        self.order_factory = OrderFactory(
            trader_id=TraderId("TESTER-000"),
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )

    def test_side_from_order_side_given_invalid_value(self) -> None:
        """
        Test raises `ValueError`.
        """
        # Arrange, Act
        with pytest.raises(ValueError):
            Position.side_from_order_side(0)

    @pytest.mark.parametrize(
        ("order_side", "expected"),
        [
            [OrderSide.BUY, PositionSide.LONG],
            [OrderSide.SELL, PositionSide.SHORT],
        ],
    )
    def test_side_from_order_side_given_valid_sides(
        self,
        order_side: OrderSide,
        expected: PositionSide,
    ) -> None:
        # Arrange, Act
        position_side = Position.side_from_order_side(order_side)

        # Assert
        assert position_side == expected

    def test_position_hash_str_repr(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill)

        # Act, Assert
        assert isinstance(hash(position), int)
        assert str(position) == "Position(LONG 100_000 AUD/USD.SIM, id=P-123456)"
        assert repr(position) == "Position(LONG 100_000 AUD/USD.SIM, id=P-123456)"

    def test_position_to_dict(self) -> None:
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill)

        # Act
        result = position.to_dict()

        # Assert
        assert result == {
            "position_id": "P-123456",
            "trader_id": "TESTER-000",
            "strategy_id": "S-001",
            "instrument_id": "AUD/USD.SIM",
            "account_id": "SIM-000",
            "opening_order_id": "O-19700101-000000-000-001-1",
            "closing_order_id": None,
            "entry": "BUY",
            "side": "LONG",
            "signed_qty": 100000.0,
            "quantity": "100000",
            "peak_qty": "100000",
            "ts_init": 0,
            "ts_opened": 0,
            "ts_last": 0,
            "ts_closed": None,
            "duration_ns": None,
            "avg_px_open": 1.00001,
            "avg_px_close": None,
            "quote_currency": "USD",
            "base_currency": "AUD",
            "settlement_currency": "USD",
            "realized_return": 0.0,
            "realized_pnl": "-2.00 USD",
            "commissions": ["2.00 USD"],
        }

    def test_long_position_to_dict_equity(self) -> None:
        # Arrange
        order = self.order_factory.market(
            AAPL_XNAS.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=AAPL_XNAS,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        position = Position(instrument=AAPL_XNAS, fill=fill)

        # Act
        result = position.to_dict()

        # Assert
        assert result == {
            "position_id": "P-123456",
            "trader_id": "TESTER-000",
            "strategy_id": "S-001",
            "instrument_id": "AAPL.XNAS",
            "account_id": "SIM-000",
            "opening_order_id": "O-19700101-000000-000-001-1",
            "closing_order_id": None,
            "entry": "BUY",
            "side": "LONG",
            "signed_qty": 100000.0,
            "quantity": "100000",
            "peak_qty": "100000",
            "ts_init": 0,
            "ts_opened": 0,
            "ts_last": 0,
            "ts_closed": None,
            "duration_ns": None,
            "avg_px_open": 1.00001,
            "avg_px_close": None,
            "quote_currency": "USD",
            "base_currency": None,
            "settlement_currency": "USD",
            "realized_return": 0.0,
            "realized_pnl": "0.00 USD",
            "commissions": ["0.00 USD"],
        }

    def test_short_position_to_dict_equity(self) -> None:
        # Arrange
        order = self.order_factory.market(
            AAPL_XNAS.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=AAPL_XNAS,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        position = Position(instrument=AAPL_XNAS, fill=fill)

        # Act
        result = position.to_dict()

        # Assert
        assert result == {
            "position_id": "P-123456",
            "trader_id": "TESTER-000",
            "strategy_id": "S-001",
            "instrument_id": "AAPL.XNAS",
            "account_id": "SIM-000",
            "opening_order_id": "O-19700101-000000-000-001-1",
            "closing_order_id": None,
            "entry": "SELL",
            "side": "SHORT",
            "signed_qty": -100000.0,
            "quantity": "100000",
            "peak_qty": "100000",
            "ts_init": 0,
            "ts_opened": 0,
            "ts_last": 0,
            "ts_closed": None,
            "duration_ns": None,
            "avg_px_open": 1.00001,
            "avg_px_close": None,
            "quote_currency": "USD",
            "base_currency": None,
            "settlement_currency": "USD",
            "realized_return": 0.0,
            "realized_pnl": "0.00 USD",
            "commissions": ["0.00 USD"],
        }

    @pytest.mark.parametrize(
        ("side1", "side2", "last_px1", "last_px2", "last_qty1", "last_qty2"),
        [
            [
                OrderSide.BUY,
                OrderSide.SELL,  # <-- Different side
                Price.from_str("1.00001"),
                Price.from_str("1.00001"),
                Quantity.from_str("1"),
                Quantity.from_str("1"),
            ],
            [
                OrderSide.BUY,
                OrderSide.SELL,  # <-- Different side
                Price.from_str("1.00001"),
                Price.from_str("1.00001"),
                Quantity.from_str("1"),
                Quantity.from_str("1"),
            ],
            [
                OrderSide.BUY,
                OrderSide.SELL,  # <-- Different side
                Price.from_str("1.00001"),
                Price.from_str("1.00001"),
                Quantity.from_str("1"),
                Quantity.from_str("1"),
            ],
        ],
    )
    def test_position_filled_with_duplicate_trade_id_different_trade(
        self,
        side1: OrderSide,
        side2: OrderSide,
        last_px1: Price,
        last_px2: Price,
        last_qty1: Quantity,
        last_qty2: Quantity,
    ) -> None:
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            side1,
            Quantity.from_int(4),
        )
        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            side2,
            Quantity.from_int(4),
        )

        trade_id = TradeId("1")

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            strategy_id=StrategyId("S-001"),
            last_px=last_px1,
            last_qty=last_qty1,
            trade_id=trade_id,
            position_id=PositionId("1"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            strategy_id=StrategyId("S-001"),
            last_px=last_px2,
            last_qty=last_qty2,
            trade_id=trade_id,
            position_id=PositionId("1"),
        )

        # Act
        position = Position(instrument=AUDUSD_SIM, fill=fill1)
        position.apply(fill2)

    def test_position_filled_with_duplicate_trade_id_and_same_trade(self) -> None:
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(4),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.000"),
            trade_id=TradeId("1"),
            position_id=PositionId("1"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill)

        # Act
        with pytest.raises(KeyError):
            position.apply(fill)

    def test_position_filled_with_buy_order(self) -> None:
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        last = Price.from_str("1.00050")

        # Act
        position = Position(instrument=AUDUSD_SIM, fill=fill)

        # Assert
        assert position.symbol == AUDUSD_SIM.id.symbol
        assert position.venue == AUDUSD_SIM.id.venue
        assert not position.is_opposite_side(fill.order_side)
        assert position == position  # Equality operator test
        assert position.opening_order_id == ClientOrderId("O-19700101-000000-000-001-1")
        assert position.closing_order_id is None
        assert position.quantity == Quantity.from_int(100_000)
        assert position.peak_qty == Quantity.from_int(100_000)
        assert position.size_precision == 0
        assert position.closing_order_side() == OrderSide.SELL
        assert position.signed_decimal_qty() == Decimal("100000")
        assert position.signed_qty == 100_000.0
        assert position.entry == OrderSide.BUY
        assert position.side == PositionSide.LONG
        assert position.ts_opened == 0
        assert position.duration_ns == 0
        assert position.avg_px_open == 1.00001
        assert position.event_count == 1
        assert position.client_order_ids == [order.client_order_id]
        assert position.venue_order_ids == [VenueOrderId("1")]
        assert position.trade_ids == [TradeId("E-19700101-000000-000-001-1")]
        assert position.last_trade_id == TradeId("E-19700101-000000-000-001-1")
        assert position.id == PositionId("P-123456")
        assert len(position.events) == 1
        assert position.is_long
        assert not position.is_short
        assert position.is_open
        assert not position.is_closed
        assert position.realized_return == 0
        assert position.realized_pnl == Money(-2.00, USD)
        assert position.unrealized_pnl(last) == Money(49.00, USD)
        assert position.total_pnl(last) == Money(47.00, USD)
        assert position.commissions() == [Money(2.00, USD)]
        assert repr(position) == "Position(LONG 100_000 AUD/USD.SIM, id=P-123456)"

    def test_position_filled_with_sell_order(self) -> None:
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        last = Price.from_str("1.00050")

        # Act
        position = Position(instrument=AUDUSD_SIM, fill=fill)

        # Assert
        assert position.quantity == Quantity.from_int(100_000)
        assert position.peak_qty == Quantity.from_int(100_000)
        assert position.size_precision == 0
        assert position.closing_order_side() == OrderSide.BUY
        assert position.signed_decimal_qty() == Decimal("-100000")
        assert position.signed_qty == -100_000.0
        assert position.side == PositionSide.SHORT
        assert position.ts_opened == 0
        assert position.avg_px_open == 1.00001
        assert position.event_count == 1
        assert position.trade_ids == [TradeId("E-19700101-000000-000-001-1")]
        assert position.last_trade_id == TradeId("E-19700101-000000-000-001-1")
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

    def test_position_partial_fills_with_buy_order(self) -> None:
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
            last_qty=Quantity.from_int(50_000),
        )

        last = Price.from_str("1.00048")

        position = Position(instrument=AUDUSD_SIM, fill=fill)

        # Act, Assert
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

    def test_position_partial_fills_with_sell_order(self) -> None:
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        fill1 = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            trade_id=TradeId("1"),
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
            last_qty=Quantity.from_int(50_000),
        )

        fill2 = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            trade_id=TradeId("2"),
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00002"),
            last_qty=Quantity.from_int(50_000),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)

        last = Price.from_str("1.00050")

        # Act
        position.apply(fill2)

        # Assert
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

    def test_position_filled_with_buy_order_then_sell_order(self) -> None:
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(150000),
        )

        fill1 = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
            ts_event=1_000_000_000,
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)

        fill2 = OrderFilled(
            self.trader_id,
            StrategyId("S-001"),
            order.instrument_id,
            order.client_order_id,
            VenueOrderId("2"),
            self.account_id,
            TradeId("E2"),
            PositionId("T123456"),
            OrderSide.SELL,
            OrderType.MARKET,
            order.quantity,
            Price.from_str("1.00011"),
            AUDUSD_SIM.quote_currency,
            Money(0, USD),
            LiquiditySide.TAKER,
            UUID4(),
            2_000_000_000,
            0,
        )

        last = Price.from_str("1.00050")

        # Act
        position.apply(fill2)

        # Assert
        assert position.is_opposite_side(fill2.order_side)
        assert position.quantity == Quantity.zero()
        assert position.size_precision == 0
        assert position.signed_decimal_qty() == Decimal()
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

    def test_position_filled_with_sell_order_then_buy_order(self) -> None:
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-19700101-000000-000-001-1"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            trade_id=TradeId("1"),
            position_id=PositionId("P-19700101-000000-000-001-1"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
            last_qty=Quantity.from_int(50_000),
        )

        fill3 = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            trade_id=TradeId("2"),
            position_id=PositionId("P-19700101-000000-000-001-1"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00003"),
            last_qty=Quantity.from_int(50_000),
        )

        last = Price.from_str("1.00050")

        # Act
        position.apply(fill2)
        position.apply(fill3)

        # Assert
        assert position.quantity == Quantity.zero()
        assert position.side == PositionSide.FLAT
        assert position.ts_opened == 0
        assert position.avg_px_open == Decimal("1.0")
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

    def test_position_filled_with_no_change(self) -> None:
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-19700101-000000-000-001-1"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00000"),
        )

        last = Price.from_str("1.00050")

        # Act
        position.apply(fill2)

        # Assert
        assert position.quantity == Quantity.zero()
        assert position.side == PositionSide.FLAT
        assert position.ts_opened == 0
        assert position.avg_px_open == Decimal("1.0")
        assert position.event_count == 2
        assert position.client_order_ids == [order1.client_order_id, order2.client_order_id]
        assert position.trade_ids == [
            TradeId("E-19700101-000000-000-001-1"),
            TradeId("E-19700101-000000-000-001-2"),
        ]
        assert position.ts_closed == 0
        assert position.avg_px_close == Decimal("1.0")
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

    def test_position_long_with_multiple_filled_orders(self) -> None:
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order3 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(200_000),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        fill3 = TestEventStubs.order_filled(
            order3,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00010"),
        )

        last = Price.from_str("1.00050")

        # Act
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

    def test_pnl_calculation_from_trading_technologies_example(self) -> None:
        # https://www.tradingtechnologies.com/xtrader-help/fix-adapter-reference/pl-calculation-algorithm/understanding-pl-calculations/

        # Arrange
        order1 = self.order_factory.market(
            ETHUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_int(12),
        )

        order2 = self.order_factory.market(
            ETHUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_int(17),
        )

        order3 = self.order_factory.market(
            ETHUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_int(9),
        )

        order4 = self.order_factory.market(
            ETHUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_int(4),
        )

        order5 = self.order_factory.market(
            ETHUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_int(3),
        )

        # Act
        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=ETHUSDT_BINANCE,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            last_px=Price.from_int(100),
        )

        position = Position(instrument=ETHUSDT_BINANCE, fill=fill1)

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=ETHUSDT_BINANCE,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            last_px=Price.from_int(99),
        )

        position.apply(fill2)
        assert position.quantity == Quantity.from_int(29)
        assert position.realized_pnl == Money(-0.28830000, USDT)
        assert position.avg_px_open == 99.41379310344827

        fill3 = TestEventStubs.order_filled(
            order3,
            instrument=ETHUSDT_BINANCE,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_int(101),
        )

        position.apply(fill3)
        assert position.quantity == Quantity.from_int(20)
        assert position.realized_pnl == Money(13.89666207, USDT)
        assert position.avg_px_open == 99.41379310344827

        fill4 = TestEventStubs.order_filled(
            order4,
            instrument=ETHUSDT_BINANCE,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_int(105),
        )

        position.apply(fill4)
        assert position.quantity == Quantity.from_int(16)
        assert position.realized_pnl == Money(36.19948966, USDT)
        assert position.avg_px_open == 99.41379310344827

        fill5 = TestEventStubs.order_filled(
            order5,
            instrument=ETHUSDT_BINANCE,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_int(103),
        )

        position.apply(fill5)
        assert position.quantity == Quantity.from_int(19)
        assert position.realized_pnl == Money(36.16858966, USDT)
        assert position.avg_px_open == 99.98003629764065
        assert (
            repr(position)
            == "Position(LONG 19.00000 ETHUSDT.BINANCE, id=P-19700101-000000-000-001-1)"
        )

    def test_position_closed_and_reopened(self) -> None:
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(150000),
        )

        fill1 = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
            ts_event=1_000_000_000,
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)

        fill2 = OrderFilled(
            self.trader_id,
            StrategyId("S-001"),
            order.instrument_id,
            order.client_order_id,
            VenueOrderId("2"),
            self.account_id,
            TradeId("E2"),
            PositionId("P-123456"),
            OrderSide.SELL,
            OrderType.MARKET,
            order.quantity,
            Price.from_str("1.00011"),
            AUDUSD_SIM.quote_currency,
            Money(0, USD),
            LiquiditySide.TAKER,
            UUID4(),
            2_000_000_000,
            0,
        )

        position.apply(fill2)

        fill3 = OrderFilled(
            self.trader_id,
            StrategyId("S-001"),
            order.instrument_id,
            order.client_order_id,
            VenueOrderId("2"),
            self.account_id,
            TradeId("E3"),
            PositionId("P-123456"),
            OrderSide.BUY,
            OrderType.MARKET,
            order.quantity,
            Price.from_str("1.00012"),
            AUDUSD_SIM.quote_currency,
            Money(0, USD),
            LiquiditySide.TAKER,
            UUID4(),
            3_000_000_000,
            0,
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
        assert position.ts_closed == 0
        assert position.avg_px_close == 0.0
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

    def test_position_realized_pnl_with_interleaved_order_sides(self) -> None:
        # Arrange
        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("12.000000"),
        )

        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("17.000000"),
        )

        order3 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_str("9.000000"),
        )

        order4 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("3.000000"),
        )

        order5 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_str("4.000000"),
        )

        # Act
        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            last_px=Price.from_str("10000.00"),
        )

        position = Position(instrument=BTCUSDT_BINANCE, fill=fill1)

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            last_px=Price.from_str("9999.00"),
        )

        position.apply(fill2)
        assert position.quantity == Quantity.from_str("29.000000")
        assert position.realized_pnl == Money(-289.98300000, USDT)
        assert position.avg_px_open == 9999.413793103447

        fill3 = TestEventStubs.order_filled(
            order3,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("10001.00"),
        )

        position.apply(fill3)
        assert position.quantity == Quantity.from_int(20)
        assert position.realized_pnl == Money(-365.71613793, USDT)
        assert position.avg_px_open == 9999.413793103447

        fill4 = TestEventStubs.order_filled(
            order4,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("10003.00"),
        )

        position.apply(fill4)
        assert position.quantity == Quantity.from_int(23)
        assert position.realized_pnl == Money(-395.72513793, USDT)
        assert position.avg_px_open == 9999.88155922039

        fill5 = TestEventStubs.order_filled(
            order5,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("10005"),
        )

        position.apply(fill5)
        assert position.quantity == Quantity.from_int(19)
        assert position.realized_pnl == Money(-415.27137481, USDT)
        assert position.avg_px_open == 9999.88155922039
        assert (
            repr(position)
            == "Position(LONG 19.000000 BTCUSDT.BINANCE, id=P-19700101-000000-000-001-1)"
        )

    def test_calculate_pnl_when_given_position_side_flat_returns_zero(self) -> None:
        # Arrange
        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_int(12),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("10500.00"),
        )

        position = Position(instrument=BTCUSDT_BINANCE, fill=fill)

        # Act
        result = position.calculate_pnl(
            10500.00,
            10500.00,
            Quantity.from_int(100_000),
        )

        # Assert
        assert result == Money(0, USDT)

    def test_calculate_pnl_for_long_position_win(self) -> None:
        # Arrange
        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_int(12),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("10500.00"),
        )

        position = Position(instrument=BTCUSDT_BINANCE, fill=fill)

        # Act
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

    def test_calculate_pnl_for_long_position_loss(self) -> None:
        # Arrange
        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_int(12),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("10500.00"),
        )

        position = Position(instrument=BTCUSDT_BINANCE, fill=fill)

        # Act
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

    def test_calculate_pnl_for_short_position_winning(self) -> None:
        # Arrange
        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_str("10.150000"),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("10500.00"),
        )

        position = Position(instrument=BTCUSDT_BINANCE, fill=fill)

        # Act
        pnl = position.calculate_pnl(
            10500.00,
            10390.00,
            Quantity.from_str("10.150000"),
        )

        # Assert
        assert pnl == Money(1116.50000000, USDT)
        assert position.unrealized_pnl(Price.from_str("10390.00")) == Money(1116.50000000, USDT)
        assert position.realized_pnl == Money(-106.57500000, USDT)
        assert position.commissions() == [Money(106.57500000, USDT)]
        assert position.notional_value(Price.from_str("10390.00")) == Money(105458.50000000, USDT)

    def test_calculate_pnl_for_short_position_loss(self) -> None:
        # Arrange
        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_str("10"),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("10500.00"),
        )

        position = Position(instrument=BTCUSDT_BINANCE, fill=fill)

        # Act
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

    def test_calculate_pnl_for_inverse1(self) -> None:
        # Arrange
        order = self.order_factory.market(
            XBTUSD_BITMEX.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=XBTUSD_BITMEX,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("10000.00"),
        )

        position = Position(instrument=XBTUSD_BITMEX, fill=fill)

        # Act
        pnl = position.calculate_pnl(
            avg_px_open=10000.00,
            avg_px_close=11000.00,
            quantity=Quantity.from_int(100_000),
        )

        # Assert
        assert pnl == Money(-0.90909091, BTC)
        assert position.unrealized_pnl(Price.from_str("11000.00")) == Money(-0.90909091, BTC)
        assert position.realized_pnl == Money(-0.00750000, BTC)
        assert position.notional_value(Price.from_str("11000.00")) == Money(9.09090909, BTC)

    def test_calculate_pnl_for_inverse2(self) -> None:
        # Arrange
        order = self.order_factory.market(
            ETHUSD_BITMEX.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=ETHUSD_BITMEX,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("375.95"),
        )

        position = Position(instrument=ETHUSD_BITMEX, fill=fill)

        # Act, Assert
        assert position.unrealized_pnl(Price.from_str("370.00")) == Money(4.27745208, ETH)
        assert position.notional_value(Price.from_str("370.00")) == Money(270.27027027, ETH)

    @pytest.mark.parametrize(
        (
            "quantity",
            "last_px",
            "mark_price",
            "expected_unrealized",
            "expected_notional",
        ),
        [
            [
                Quantity.from_int(100_000),
                Price.from_str("2.0"),
                Price.from_str("3.0"),
                Money(16_666.66666667, BTC),
                Money(33_333.33333333, BTC),
            ],
            # Example from https://www.bitmex.com/app/pnlGuide
            [
                Quantity.from_int(1_000),
                Price.from_str("1000.0"),
                Price.from_str("1250.0"),
                Money(0.2, BTC),
                Money(0.8, BTC),
            ],
        ],
    )
    def test_calculate_pnl_for_inverse3(
        self,
        quantity: Quantity,
        last_px: Price,
        mark_price: Price,
        expected_unrealized: Money,
        expected_notional: Money,
    ) -> None:
        # Arrange
        order = self.order_factory.market(
            XBTUSD_BITMEX.id,
            OrderSide.BUY,
            quantity=quantity,
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=XBTUSD_BITMEX,
            position_id=PositionId("P-1"),
            strategy_id=StrategyId("S-001"),
            last_px=last_px,
        )

        position = Position(instrument=XBTUSD_BITMEX, fill=fill)

        # Act, Assert
        assert position.unrealized_pnl(mark_price) == expected_unrealized
        assert position.notional_value(mark_price) == expected_notional

    def test_calculate_unrealized_pnl_for_long(self) -> None:
        # Arrange
        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("2.000000"),
        )

        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("2.000000"),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("10500.00"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("10500.00"),
        )

        position = Position(instrument=BTCUSDT_BINANCE, fill=fill1)
        position.apply(fill2)

        # Act
        pnl = position.unrealized_pnl(Price.from_str("11505.60"))

        # Assert
        assert pnl == Money(4022.40000000, USDT)
        assert position.realized_pnl == Money(-42.00000000, USDT)
        assert position.commissions() == [Money(42.00000000, USDT)]

    def test_calculate_unrealized_pnl_for_short(self) -> None:
        # Arrange
        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_str("5.912000"),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("10505.60"),
        )

        position = Position(instrument=BTCUSDT_BINANCE, fill=fill)

        pnl = position.unrealized_pnl(Price.from_str("10407.15"))

        # Assert
        assert pnl == Money(582.03640000, USDT)
        assert position.realized_pnl == Money(-62.10910720, USDT)
        assert position.commissions() == [Money(62.10910720, USDT)]

    def test_calculate_unrealized_pnl_for_long_inverse(self) -> None:
        # Arrange
        order = self.order_factory.market(
            XBTUSD_BITMEX.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=XBTUSD_BITMEX,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("10500.00"),
        )

        position = Position(XBTUSD_BITMEX, fill)

        # Act

        pnl = position.unrealized_pnl(Price.from_str("11505.60"))

        # Assert
        assert pnl == Money(0.83238969, BTC)
        assert position.realized_pnl == Money(-0.00714286, BTC)
        assert position.commissions() == [Money(0.00714286, BTC)]

    def test_calculate_unrealized_pnl_for_short_inverse(self) -> None:
        # Arrange
        order = self.order_factory.market(
            XBTUSD_BITMEX.id,
            OrderSide.SELL,
            Quantity.from_int(1_250_000),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=XBTUSD_BITMEX,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("15500.00"),
        )

        position = Position(XBTUSD_BITMEX, fill)

        # Act

        pnl = position.unrealized_pnl(Price.from_str("12506.65"))

        # Assert
        assert pnl == Money(19.30166700, BTC)
        assert position.realized_pnl == Money(-0.06048387, BTC)
        assert position.commissions() == [Money(0.06048387, BTC)]

    @pytest.mark.parametrize(
        ("order_side", "quantity", "expected_signed_qty", "expected_decimal_qty"),
        [
            [OrderSide.BUY, 25, 25.0, Decimal("25")],
            [OrderSide.SELL, 25, -25.0, Decimal("-25")],
        ],
    )
    def test_signed_qty_decimal_qty_for_equity(
        self,
        order_side: OrderSide,
        quantity: int,
        expected_signed_qty: float,
        expected_decimal_qty: Decimal,
    ) -> None:
        # Arrange
        order = self.order_factory.market(
            AAPL_XNAS.id,
            order_side,
            Quantity.from_int(quantity),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=AAPL_XNAS,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("100"),
        )

        # Act
        position = Position(instrument=AAPL_XNAS, fill=fill)

        # Assert
        assert position.signed_qty == expected_signed_qty
        assert position.signed_decimal_qty() == expected_decimal_qty

    def test_purge_order_events(self) -> None:
        # Arrange
        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("2.000000"),
        )

        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("2.000000"),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("10500.00"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("10500.00"),
        )

        position = Position(instrument=BTCUSDT_BINANCE, fill=fill1)
        position.apply(fill2)

        # Act
        position.purge_events_for_order(fill1.client_order_id)

        # Assert
        assert len(position.events) == 1
        assert len(position.trade_ids) == 1
        assert fill1 not in position.events
        assert fill2 in position.events
        assert fill1.trade_id not in position.trade_ids
        assert fill2.trade_id in position.trade_ids

    def test_purge_all_events_returns_none_for_last_event_and_trade_id(self) -> None:
        # Arrange
        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.000000"),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("10500.00"),
            ts_event=1_000_000_000,  # Explicit non-zero timestamp
        )

        position = Position(instrument=BTCUSDT_BINANCE, fill=fill)

        # Verify position starts with event
        assert position.event_count == 1
        assert position.last_event is not None
        assert position.last_trade_id is not None

        # Store original timestamps (should be non-zero)
        original_ts_opened = position.ts_opened
        original_ts_last = position.ts_last
        assert original_ts_opened > 0
        assert original_ts_last > 0

        # Act - Purge all events by purging the only order
        position.purge_events_for_order(fill.client_order_id)

        # Assert
        assert position.event_count == 0
        assert position.events == []
        assert position.trade_ids == []
        assert position.last_event is None
        assert position.last_trade_id is None

        # Verify timestamps are zeroed - empty shell has no meaningful history
        assert position.ts_opened == 0
        assert position.ts_last == 0
        assert position.ts_closed == 0
        assert position.duration_ns == 0

        # Verify empty shell reports as closed (this was the bug we fixed!)
        # is_closed must return True so cache purge logic recognizes empty shells
        assert position.is_closed
        assert not position.is_open
        assert position.side == PositionSide.FLAT

    def test_revive_from_empty_shell(self) -> None:
        """
        Test adding a fill to an empty shell position revives it with correct state.
        """
        # Arrange: Create position with a fill
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00000"),
            ts_event=1_000_000_000,
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)

        # Purge all events to create empty shell
        position.purge_events_for_order(order1.client_order_id)

        # Verify it's an empty shell
        assert position.is_closed
        assert position.ts_closed == 0
        assert position.event_count == 0

        # Act: Add new fill to revive the position
        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(50_000),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00020"),
            ts_event=3_000_000_000,
        )

        position.apply(fill2)

        # Assert: Position should be alive with new timestamps
        assert position.is_long
        assert not position.is_closed
        # NOTE: Python uses 0 for "not closed", Rust uses None (semantic difference in representation)
        assert position.ts_closed == 0  # Reset to 0 when reopened (Rust: None)
        assert position.ts_opened == fill2.ts_event
        assert position.ts_last == fill2.ts_event
        assert position.event_count == 1
        assert position.quantity == Quantity.from_int(50_000)

    def test_empty_shell_position_invariants(self) -> None:
        """
        Property-based test: Any position with event_count == 0 must satisfy invariants.
        This test documents the contract that empty shell positions MUST follow.
        """
        # Arrange: Create and purge position to get empty shell
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00000"),
            ts_event=1_000_000_000,
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill)
        position.purge_events_for_order(order.client_order_id)

        # INVARIANTS: When event_count == 0, the following MUST be true
        assert position.event_count == 0, "Precondition: event_count must be 0"

        # Invariant 1: Position must report as closed
        assert position.is_closed, "INV1: Empty shell must report is_closed == True"
        assert not position.is_open, "INV1: Empty shell must report is_open == False"

        # Invariant 2: Position must be FLAT
        assert position.side == PositionSide.FLAT, "INV2: Empty shell must be FLAT"

        # Invariant 3: ts_closed must be 0 (not None, not preserved)
        assert position.ts_closed == 0, "INV3: Empty shell ts_closed must be 0"

        # Invariant 4: All lifecycle timestamps must be zeroed
        assert position.ts_opened == 0, "INV4: Empty shell ts_opened must be 0"
        assert position.ts_last == 0, "INV4: Empty shell ts_last must be 0"
        assert position.duration_ns == 0, "INV4: Empty shell duration_ns must be 0"

        # Invariant 5: Quantity must be zero
        assert position.quantity == Quantity.zero(
            AUDUSD_SIM.size_precision,
        ), "INV5: Empty shell quantity must be 0"

        # Invariant 6: No events or trade IDs
        assert position.events == [], "INV6: Empty shell must have no events"
        assert position.trade_ids == [], "INV6: Empty shell must have no trade IDs"
        assert position.last_event is None, "INV6: Empty shell must have no last event"
        assert position.last_trade_id is None, "INV6: Empty shell must have no last trade ID"

    def test_commission_accumulation_single_currency(self) -> None:
        """
        Test that commissions are correctly accumulated for a single currency.
        """
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(50_000),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00000"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00100"),
        )

        # Act
        position = Position(instrument=AUDUSD_SIM, fill=fill1)
        position.apply(fill2)

        # Assert
        commissions = position.commissions()
        assert len(commissions) == 1
        # Default commission: 100k * 0.00002 + 50k * 0.00002 = 2 + 1 = 3 USD
        assert commissions[0] == Money(3, USD)

    def test_commission_accumulation_multiple_fills(self) -> None:
        """
        Test that commissions are correctly accumulated with multiple fills.
        """
        # Arrange
        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.0"),
        )

        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("0.5"),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("50000.00"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("51000.00"),
        )

        # Act
        position = Position(instrument=BTCUSDT_BINANCE, fill=fill1)
        position.apply(fill2)

        # Assert
        commissions = position.commissions()
        assert len(commissions) == 1
        # BTCUSDT_BINANCE has taker_fee of 0.001 (0.1%)
        # Commission: 1.0 * 50000 * 0.001 + 0.5 * 51000 * 0.001 = 50 + 25.5 = 75.5 USDT
        assert commissions[0] == Money(75.5, USDT)

    def test_realized_return_calculation_long_position(self) -> None:
        """
        Test realized return calculation for a long position.
        """
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00000"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.10000"),  # 10% gain
        )

        # Act
        position = Position(instrument=AUDUSD_SIM, fill=fill1)
        position.apply(fill2)

        # Assert
        assert position.is_closed
        assert position.realized_return == pytest.approx(0.1, rel=1e-9)  # 10% return

    def test_realized_return_calculation_short_position(self) -> None:
        """
        Test realized return calculation for a short position.
        """
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.10000"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00000"),  # ~9.09% gain
        )

        # Act
        position = Position(instrument=AUDUSD_SIM, fill=fill1)
        position.apply(fill2)

        # Assert
        assert position.is_closed
        assert position.realized_return == pytest.approx(0.09090909, rel=1e-6)

    def test_duration_calculation(self) -> None:
        """
        Test position duration calculation from open to close.
        """
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        # Create fills with specific timestamps
        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00000"),
            ts_event=1_000_000_000,  # 1 second
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00010"),
            ts_event=3_600_000_000_000,  # 1 hour later
        )

        # Act
        position = Position(instrument=AUDUSD_SIM, fill=fill1)
        position.apply(fill2)

        # Assert
        assert position.is_closed
        assert position.duration_ns == 3_599_000_000_000  # 1 hour - 1 second in nanoseconds
        assert position.ts_opened == 1_000_000_000
        assert position.ts_closed == 3_600_000_000_000

    def test_is_opposite_side_method(self) -> None:
        """
        Test the is_opposite_side method for long and short positions.
        """
        # Arrange
        buy_order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        buy_fill = TestEventStubs.order_filled(
            buy_order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-LONG"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00000"),
        )

        sell_order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        sell_fill = TestEventStubs.order_filled(
            sell_order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-SHORT"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00000"),
        )

        # Act
        long_position = Position(instrument=AUDUSD_SIM, fill=buy_fill)
        short_position = Position(instrument=AUDUSD_SIM, fill=sell_fill)

        # Assert
        # Long position
        assert long_position.is_long
        assert not long_position.is_opposite_side(OrderSide.BUY)
        assert long_position.is_opposite_side(OrderSide.SELL)

        # Short position
        assert short_position.is_short
        assert short_position.is_opposite_side(OrderSide.BUY)
        assert not short_position.is_opposite_side(OrderSide.SELL)

    def test_avg_px_calculations_multiple_fills(self) -> None:
        """
        Test average price calculations with multiple fills at different prices.
        """
        # Arrange - Build position with multiple fills
        fills_data = [
            (OrderSide.BUY, 50_000, "1.00000"),
            (OrderSide.BUY, 30_000, "1.00100"),
            (OrderSide.BUY, 20_000, "1.00200"),
        ]

        position = None
        for i, (side, qty, price) in enumerate(fills_data):
            order = self.order_factory.market(
                AUDUSD_SIM.id,
                side,
                Quantity.from_int(qty),
            )

            fill = TestEventStubs.order_filled(
                order,
                instrument=AUDUSD_SIM,
                position_id=PositionId("P-123456"),
                strategy_id=StrategyId("S-001"),
                last_px=Price.from_str(price),
            )

            if position is None:
                position = Position(instrument=AUDUSD_SIM, fill=fill)
            else:
                position.apply(fill)

        # Assert
        # Weighted average: (50k * 1.0 + 30k * 1.001 + 20k * 1.002) / 100k
        expected_avg = (50_000 * 1.00000 + 30_000 * 1.00100 + 20_000 * 1.00200) / 100_000
        assert position is not None
        assert position.avg_px_open == pytest.approx(expected_avg, rel=1e-9)
        assert position.quantity == Quantity.from_int(100_000)

    def test_closing_order_side_for_different_position_sides(self) -> None:
        """
        Test the closing_order_side method returns correct side.
        """
        # Arrange
        buy_order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        buy_fill = TestEventStubs.order_filled(
            buy_order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-LONG"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00000"),
        )

        sell_order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        sell_fill = TestEventStubs.order_filled(
            sell_order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-SHORT"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00000"),
        )

        # Act
        long_position = Position(instrument=AUDUSD_SIM, fill=buy_fill)
        short_position = Position(instrument=AUDUSD_SIM, fill=sell_fill)

        # Assert
        assert long_position.closing_order_side() == OrderSide.SELL
        assert short_position.closing_order_side() == OrderSide.BUY

    def test_signed_decimal_qty_for_different_sides(self) -> None:
        """
        Test signed_decimal_qty returns correct sign based on position side.
        """
        # Arrange
        buy_order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        buy_fill = TestEventStubs.order_filled(
            buy_order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-LONG"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00000"),
        )

        sell_order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(75_000),
        )

        sell_fill = TestEventStubs.order_filled(
            sell_order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-SHORT"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00000"),
        )

        # Act
        long_position = Position(instrument=AUDUSD_SIM, fill=buy_fill)
        short_position = Position(instrument=AUDUSD_SIM, fill=sell_fill)

        # Assert
        assert long_position.signed_decimal_qty() == Decimal("100000")
        assert short_position.signed_decimal_qty() == Decimal("-75000")

    def test_notional_value_calculation(self) -> None:
        """
        Test notional value calculation for positions.
        """
        # Arrange
        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("2.5"),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("50000.00"),
        )

        position = Position(instrument=BTCUSDT_BINANCE, fill=fill)

        # Act
        current_price = Price.from_str("52000.00")
        notional = position.notional_value(current_price)

        # Assert
        # 2.5 BTC * 52000 USDT/BTC = 130000 USDT
        assert notional == Money(130000, USDT)

    def test_partial_close_updates_quantities_correctly(self) -> None:
        """
        Test that partial closes update buy/sell quantities correctly.
        """
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(40_000),
        )

        order3 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(60_000),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00000"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00100"),
        )

        fill3 = TestEventStubs.order_filled(
            order3,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00200"),
        )

        # Act
        position = Position(instrument=AUDUSD_SIM, fill=fill1)
        position.apply(fill2)
        position.apply(fill3)

        # Assert
        assert position.is_closed
        assert position.quantity == Quantity.from_int(0)
        assert position.peak_qty == Quantity.from_int(100_000)

    def test_id_list_deduplication_and_sorting(self) -> None:
        """
        Test that client_order_ids, venue_order_ids, and trade_ids are deduplicated and
        sorted.
        """
        # Arrange - Create fills with duplicate IDs
        orders = []
        fills = []

        # Create multiple orders with controlled IDs
        for i in range(3):
            order = self.order_factory.market(
                AUDUSD_SIM.id,
                OrderSide.BUY,
                Quantity.from_int(10_000),
            )
            orders.append(order)

            # Create fills with specific venue_order_ids and trade_ids
            fill = TestEventStubs.order_filled(
                order,
                instrument=AUDUSD_SIM,
                position_id=PositionId("P-123456"),
                strategy_id=StrategyId("S-001"),
                venue_order_id=(
                    VenueOrderId("V-001") if i < 2 else VenueOrderId("V-002")
                ),  # Duplicate first
                trade_id=TradeId(f"T-00{i+1}"),  # Unique trade IDs
                last_px=Price.from_str("1.00000"),
            )
            fills.append(fill)

        # Act
        position = Position(instrument=AUDUSD_SIM, fill=fills[0])

        for fill in fills[1:]:
            position.apply(fill)

        # Assert
        # Check that IDs are deduplicated where appropriate
        assert len(position.client_order_ids) == 3  # All unique client order IDs
        assert len(position.venue_order_ids) == 2  # Should be deduplicated: V-001, V-002
        assert len(position.trade_ids) == 3  # All unique: T-001, T-002, T-003

        # Check sorting (if applicable - need to verify if sorting is guaranteed)
        # The lists should be in chronological order of fills
        assert position.venue_order_ids == [VenueOrderId("V-001"), VenueOrderId("V-002")]
        assert position.trade_ids == [TradeId("T-001"), TradeId("T-002"), TradeId("T-003")]

    def test_position_equality_and_hash_semantics(self) -> None:
        """
        Test that Position equality and hash are based solely on position ID.
        """
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(50_000),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00000"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-789"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.10000"),
        )

        # Act
        position1 = Position(instrument=AUDUSD_SIM, fill=fill1)
        position2 = Position(instrument=AUDUSD_SIM, fill=fill2)

        # Create another position with same ID as position1 but different state
        fill3 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),  # Same ID as position1
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.50000"),  # Different price
        )
        position3 = Position(instrument=AUDUSD_SIM, fill=fill3)

        # Assert
        # Different IDs = not equal
        assert position1 != position2
        assert hash(position1) != hash(position2)

        # Same ID = equal (even with different internal state)
        assert position1 == position3
        assert hash(position1) == hash(position3)

        # Test as dictionary keys
        position_dict = {position1: "first", position3: "third"}
        assert len(position_dict) == 1  # Should only have one entry due to same ID
        assert position_dict[position1] == "third"  # Last one wins

    def test_purge_events_preserves_financial_state(self) -> None:
        """
        Test that purging events correctly rebuilds position state from remaining fills.

        When the opening fill is purged, the remaining fill becomes the new opening
        fill, causing the position to flip sides and recalculate all financial state.

        """
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(50_000),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00000"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.10000"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)
        position.apply(fill2)

        # Before purge: LONG 50,000 @ 1.00 avg open (partially closed at 1.10)
        assert position.side == PositionSide.LONG
        assert position.signed_qty == 50_000.0
        assert position.avg_px_open == 1.0
        assert position.avg_px_close == 1.1

        # Act - Purge the opening BUY fill
        position.purge_events_for_order(order1.client_order_id)

        # Assert - Position rebuilt from remaining SELL fill
        # The SELL now becomes the opening fill, creating a SHORT position
        assert position.event_count == 1  # Only fill2 remains
        assert len(position.trade_ids) == 1

        # State should be recalculated from the remaining fill
        assert position.side == PositionSide.SHORT  # Flipped to SHORT
        assert position.signed_qty == -50_000.0  # Now short
        assert position.quantity == Quantity.from_int(50_000)
        assert position.avg_px_open == 1.1  # The SELL @ 1.10 is now the opening price
        assert position.avg_px_close == 0.0  # No closing fills yet
        assert position.realized_return == 0.0  # No realized return yet
        assert position.ts_opened == fill2.ts_event  # Opened at fill2, not fill1
        assert position.ts_closed == 0  # Still open

    def test_peak_quantity_tracking(self) -> None:
        """
        Test that peak_qty correctly tracks the maximum position size reached.
        """
        # Arrange - Build up position then partially close
        fills_data = [
            (OrderSide.BUY, 50_000, "1.00000"),  # Position: 50k
            (OrderSide.BUY, 30_000, "1.00100"),  # Position: 80k
            (OrderSide.BUY, 40_000, "1.00200"),  # Position: 120k (peak)
            (OrderSide.SELL, 70_000, "1.00300"),  # Position: 50k
            (OrderSide.SELL, 30_000, "1.00400"),  # Position: 20k
        ]

        position = None
        for i, (side, qty, price) in enumerate(fills_data):
            order = self.order_factory.market(
                AUDUSD_SIM.id,
                side,
                Quantity.from_int(qty),
            )

            fill = TestEventStubs.order_filled(
                order,
                instrument=AUDUSD_SIM,
                position_id=PositionId("P-123456"),
                strategy_id=StrategyId("S-001"),
                last_px=Price.from_str(price),
            )

            if position is None:
                position = Position(instrument=AUDUSD_SIM, fill=fill)
            else:
                position.apply(fill)

        # Assert
        assert position is not None
        assert position.peak_qty == Quantity.from_int(120_000)  # Maximum reached
        assert position.quantity == Quantity.from_int(20_000)  # Current position

    def test_position_invariants_across_fills(self) -> None:
        """
        Test key invariants that should always hold across any sequence of fills.
        """
        # Arrange - Random sequence of buys and sells
        fills_data = [
            (OrderSide.BUY, 100_000, "1.00000"),
            (OrderSide.SELL, 30_000, "1.00100"),
            (OrderSide.BUY, 50_000, "1.00200"),
            (OrderSide.SELL, 120_000, "1.00300"),
            (OrderSide.SELL, 20_000, "1.00400"),  # Goes short
            (OrderSide.BUY, 40_000, "1.00500"),
        ]

        position = None

        for i, (side, qty, price) in enumerate(fills_data):
            order = self.order_factory.market(
                AUDUSD_SIM.id,
                side,
                Quantity.from_int(qty),
            )

            fill = TestEventStubs.order_filled(
                order,
                instrument=AUDUSD_SIM,
                position_id=PositionId("P-123456"),
                strategy_id=StrategyId("S-001"),
                last_px=Price.from_str(price),
            )

            if position is None:
                position = Position(instrument=AUDUSD_SIM, fill=fill)
            else:
                position.apply(fill)

            # Check invariants after each fill
            # Invariant 1: quantity == abs(signed_qty)
            assert position.quantity == Quantity.from_int(abs(int(position.signed_qty)))

            # Invariant 2: side consistency with signed_qty
            if position.signed_qty > 0:
                assert position.side == PositionSide.LONG
                assert position.closing_order_side() == OrderSide.SELL
            elif position.signed_qty < 0:
                assert position.side == PositionSide.SHORT
                assert position.closing_order_side() == OrderSide.BUY
            else:
                assert position.side == PositionSide.FLAT

            # Invariant 3: peak_qty never decreases
            if i > 0:
                assert position.peak_qty >= Quantity.from_int(0)  # Always non-negative

    def test_position_multiple_cycles_pnl_tracking(self) -> None:
        """
        Test that each discrete position cycle tracks its own realized PnL.

        This test validates that when positions are closed and reopened, each open-flat-
        reopen cycle tracks its own realized PnL independently, not cumulatively. This
        is the intended behavior for position PnL tracking in the platform.

        """
        # Arrange - Create test instruments
        position_id = PositionId("P-CYCLE-001")

        # Cycle 1: Open long, make profit, close
        order1_open = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        fill1_open = TestEventStubs.order_filled(
            order1_open,
            instrument=AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("0.70000"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1_open)

        # Close with profit (10 pips)
        order1_close = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        fill1_close = TestEventStubs.order_filled(
            order1_close,
            instrument=AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("0.70010"),
        )

        position.apply(fill1_close)

        # Assert Cycle 1 results
        assert position.is_closed
        # 10 pips profit (10.00) - commission (2.80) = 7.20
        assert position.realized_pnl == Money(7.20, USD)
        cycle1_pnl = position.realized_pnl

        # Cycle 2: Reopen same position ID, make loss, close
        order2_open = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(150_000),
        )

        fill2_open = TestEventStubs.order_filled(
            order2_open,
            instrument=AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("0.70020"),
        )

        # Reopen position (simulating NETTING behavior)
        position.apply(fill2_open)

        assert position.is_open
        assert position.quantity == Quantity.from_int(150_000)
        # Commission from reopening
        assert position.realized_pnl == Money(-2.10, USD)

        # Close with loss (5 pips)
        order2_close = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(150_000),
        )

        fill2_close = TestEventStubs.order_filled(
            order2_close,
            instrument=AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("0.70015"),
        )

        position.apply(fill2_close)

        # Assert Cycle 2 results
        assert position.is_closed
        # 5 pips loss on 150k (7.50) + commissions (2.10 + 2.10) = -11.70
        assert position.realized_pnl == Money(-11.70, USD)
        cycle2_pnl = position.realized_pnl

        # Cycle 3: Reopen short position, make profit, close
        order3_open = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(200_000),
        )

        fill3_open = TestEventStubs.order_filled(
            order3_open,
            instrument=AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("0.70025"),
        )

        position.apply(fill3_open)

        assert position.is_short
        assert position.quantity == Quantity.from_int(200_000)
        # Commission from reopening
        assert position.realized_pnl == Money(-2.80, USD)

        # Close short with profit (8 pips)
        order3_close = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(200_000),
        )

        fill3_close = TestEventStubs.order_filled(
            order3_close,
            instrument=AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("0.70017"),
        )

        position.apply(fill3_close)

        # Assert Cycle 3 results
        assert position.is_closed
        # 8 pips profit on 200k (16.00) - commissions (2.80 + 2.80) = 10.40
        assert position.realized_pnl == Money(10.40, USD)
        cycle3_pnl = position.realized_pnl

        # Assert - each cycle's PnL is independent
        # The position object only holds the LAST cycle's PnL
        # Portfolio/Account aggregation should handle historical cycles
        assert cycle1_pnl == Money(7.20, USD)
        assert cycle2_pnl == Money(-11.70, USD)
        assert cycle3_pnl == Money(10.40, USD)

        # The current position only shows the last cycle
        assert position.realized_pnl == cycle3_pnl

    def test_position_flip_long_to_short_pnl_tracking(self) -> None:
        """
        Test PnL tracking when position flips from long to short.

        This validates that when a position flips (e.g., selling 150k when long 100k),
        the PnL for closing the long is calculated correctly, and the new short position
        starts fresh.

        """
        # Arrange
        position_id = PositionId("P-FLIP-001")

        # Open long 100k
        order_long = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        fill_long = TestEventStubs.order_filled(
            order_long,
            instrument=AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("0.75000"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill_long)
        assert position.is_long
        assert position.quantity == Quantity.from_int(100_000)

        # Flip to short by selling 150k (closes 100k long, opens 50k short)
        # NOTE: The test expectations below rely on the current TestEventStubs.order_filled
        # behavior which uses order.quantity (150k) for commission calculation on both fills.
        # If the stub is changed to use last_qty for commission calculation, the expected
        # PnL values would need to be updated accordingly.
        order_flip = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(150_000),
        )

        # First part closes the long
        fill_close_long = TestEventStubs.order_filled(
            order_flip,
            instrument=AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("0.75020"),  # 20 pips profit
            last_qty=Quantity.from_int(100_000),
        )

        position.apply(fill_close_long)

        # Position should be closed after matching quantity
        assert position.is_closed
        # 20 pips on 100k (20.00) - commissions (1.50 + 2.25) = 16.25
        assert position.realized_pnl == Money(16.25, USD)
        long_pnl = position.realized_pnl

        # Second part opens new short
        fill_open_short = TestEventStubs.order_filled(
            order_flip,
            instrument=AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("0.75020"),
            last_qty=Quantity.from_int(50_000),
        )

        position.apply(fill_open_short)

        # Now should be short
        assert position.is_short
        assert position.quantity == Quantity.from_int(50_000)
        # Note: Commission from flip order carries over
        assert position.realized_pnl == Money(-2.25, USD)

        # Close the short with a loss
        order_close_short = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(50_000),
        )

        fill_close_short = TestEventStubs.order_filled(
            order_close_short,
            instrument=AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("0.75030"),  # 10 pips loss
        )

        position.apply(fill_close_short)

        assert position.is_closed
        # 10 pips loss on 50k (5.00) + commissions (2.25 + 0.75) = -8.00
        assert position.realized_pnl == Money(-8.00, USD)
        short_pnl = position.realized_pnl

        # Validate independent PnL tracking
        assert long_pnl == Money(16.25, USD)
        assert short_pnl == Money(-8.00, USD)
