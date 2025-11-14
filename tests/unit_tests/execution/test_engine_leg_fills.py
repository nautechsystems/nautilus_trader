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
"""
Tests for execution engine handling of leg fills from option spread orders.

This module tests the execution engine's ability to handle leg fills that don't have
corresponding orders in the cache, which occurs when spread orders are executed and
generate individual leg fills for portfolio tracking.

"""

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments import OptionContract
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.mocks.cache_database import MockCacheDatabase
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestExecutionEngineLegFills:
    """
    Test execution engine handling of leg fills from option spread orders.
    """

    def setup_method(self):
        """
        Set up test fixtures.
        """
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()
        self.strategy_id = TestIdStubs.strategy_id()
        self.account_id = TestIdStubs.account_id()

        # Create message bus and cache
        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = Cache(database=MockCacheDatabase())

        # Create portfolio
        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Create execution engine
        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Create test instruments - use different option contracts for call and put
        from nautilus_trader.model.enums import AssetClass
        from nautilus_trader.model.enums import OptionKind
        from nautilus_trader.model.identifiers import InstrumentId
        from nautilus_trader.model.identifiers import Symbol
        from nautilus_trader.model.identifiers import Venue
        from nautilus_trader.model.objects import Currency
        from nautilus_trader.model.objects import Price
        from nautilus_trader.model.objects import Quantity

        # Create call option
        self.call_option = OptionContract(
            instrument_id=InstrumentId(Symbol("E1AQ5 C6400"), Venue("XCME")),
            raw_symbol=Symbol("E1AQ5 C6400"),
            asset_class=AssetClass.EQUITY,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            underlying="E1AQ5",
            option_kind=OptionKind.CALL,
            activation_ns=0,
            expiration_ns=1640995200000000000,  # 2022-01-01
            strike_price=Price.from_str("6400.0"),
            ts_event=0,
            ts_init=0,
        )

        # Create put option
        self.put_option = OptionContract(
            instrument_id=InstrumentId(Symbol("E1AQ5 P6440"), Venue("XCME")),
            raw_symbol=Symbol("E1AQ5 P6440"),
            asset_class=AssetClass.EQUITY,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            underlying="E1AQ5",
            option_kind=OptionKind.PUT,
            activation_ns=0,
            expiration_ns=1640995200000000000,  # 2022-01-01
            strike_price=Price.from_str("6440.0"),
            ts_event=0,
            ts_init=0,
        )

        # For testing purposes, we don't need a real option spread instrument
        # since the tests focus on leg fills for individual option instruments
        self.option_spread = None

        # Add instruments to cache
        self.cache.add_instrument(self.call_option)
        self.cache.add_instrument(self.put_option)

        # Add account
        account = TestEventStubs.margin_account_state(account_id=self.account_id)
        self.portfolio.update_account(account)

    def create_leg_fill(
        self,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        trade_id: TradeId,
        side: OrderSide,
        quantity: int,
        price: float,
        position_id: PositionId | None = None,
    ) -> OrderFilled:
        """
        Create a leg fill event for testing.
        """
        return OrderFilled(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            account_id=self.account_id,
            trade_id=trade_id,
            position_id=position_id,
            order_side=side,
            order_type=OrderType.MARKET,
            last_qty=Quantity.from_int(quantity),
            last_px=Price.from_str(str(price)),
            currency=USD,
            commission=Money.from_str("1.00 USD"),
            liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
            event_id=UUID4(),
            ts_event=self.clock.timestamp_ns(),
            ts_init=self.clock.timestamp_ns(),
        )

    def test_handle_leg_fill_without_order_creates_position(self):
        """
        Test that leg fills without corresponding orders create positions.
        """
        # Arrange
        leg_fill = self.create_leg_fill(
            instrument_id=self.call_option.id,
            client_order_id=ClientOrderId("O-20250725-080046-TEST-000-1-LEG-E1AQ5 C6400"),
            venue_order_id=VenueOrderId("213-LEG-0"),
            trade_id=TradeId("0000e1a7.6882c67b.03.01-0"),
            side=OrderSide.BUY,
            quantity=3,
            price=52.75,
        )

        # Act
        self.exec_engine._handle_leg_fill_without_order(leg_fill)

        # Assert
        positions = self.cache.positions()
        assert len(positions) == 1

        position = positions[0]
        assert position.instrument_id == self.call_option.id
        assert position.side.name == "LONG"
        assert position.quantity == Quantity.from_int(3)
        assert position.avg_px_open == Price.from_str("52.75")

    def test_handle_leg_fill_without_order_updates_existing_position(self):
        """
        Test that leg fills update existing positions.
        """
        # Arrange - Create initial position
        initial_fill = self.create_leg_fill(
            instrument_id=self.put_option.id,
            client_order_id=ClientOrderId("O-20250725-080046-TEST-000-1-LEG-E1AQ5 P6440"),
            venue_order_id=VenueOrderId("213-LEG-1"),
            trade_id=TradeId("0000e1a7.6882c67b.02.01-1"),
            side=OrderSide.BUY,
            quantity=6,
            price=66.75,
        )

        self.exec_engine._handle_leg_fill_without_order(initial_fill)

        # Act - Add to existing position
        additional_fill = self.create_leg_fill(
            instrument_id=self.put_option.id,
            client_order_id=ClientOrderId("O-20250725-080046-TEST-000-2-LEG-E1AQ5 P6440"),
            venue_order_id=VenueOrderId("214-LEG-1"),
            trade_id=TradeId("0000e1a7.6882c67b.04.01-1"),
            side=OrderSide.BUY,
            quantity=3,
            price=67.25,
        )

        self.exec_engine._handle_leg_fill_without_order(additional_fill)

        # Assert
        positions = self.cache.positions()
        assert len(positions) == 1

        position = positions[0]
        assert position.instrument_id == self.put_option.id
        assert position.quantity == Quantity.from_int(9)  # 6 + 3

    def test_handle_leg_fill_without_order_missing_instrument(self):
        """
        Test handling of leg fill when instrument is not in cache.
        """
        # Arrange
        unknown_instrument_id = InstrumentId.from_str("UNKNOWN.XCME")
        leg_fill = self.create_leg_fill(
            instrument_id=unknown_instrument_id,
            client_order_id=ClientOrderId("O-20250725-080046-TEST-000-1-LEG-UNKNOWN"),
            venue_order_id=VenueOrderId("213-LEG-0"),
            trade_id=TradeId("0000e1a7.6882c67b.03.01-0"),
            side=OrderSide.BUY,
            quantity=3,
            price=52.75,
        )

        # Act
        self.exec_engine._handle_leg_fill_without_order(leg_fill)

        # Assert - No position should be created
        positions = self.cache.positions()
        assert len(positions) == 0

    def test_process_leg_fill_when_order_not_found(self):
        """
        Test that leg fills are processed when order is not found in cache.
        """
        # Arrange
        leg_fill = self.create_leg_fill(
            instrument_id=self.call_option.id,
            client_order_id=ClientOrderId("O-20250725-080046-TEST-000-1-LEG-E1AQ5 C6400"),
            venue_order_id=VenueOrderId("213-LEG-0"),
            trade_id=TradeId("0000e1a7.6882c67b.03.01-0"),
            side=OrderSide.BUY,
            quantity=3,
            price=52.75,
        )

        # Act - Process through main event handler
        self.exec_engine.process(leg_fill)

        # Assert
        positions = self.cache.positions()
        assert len(positions) == 1

        position = positions[0]
        assert position.instrument_id == self.call_option.id
        assert position.quantity == Quantity.from_int(3)

    def test_ratio_spread_leg_fills_create_separate_positions(self):
        """
        Test that 1x2 ratio spread leg fills create separate positions for each leg.
        """
        # Arrange - Create leg fills for 1x2 ratio spread (3 spread units)
        call_leg_fill = self.create_leg_fill(
            instrument_id=self.call_option.id,
            client_order_id=ClientOrderId("O-20250725-080046-TEST-000-1-LEG-E1AQ5 C6400"),
            venue_order_id=VenueOrderId("213-LEG-0"),
            trade_id=TradeId("0000e1a7.6882c67b.03.01-0"),
            side=OrderSide.BUY,
            quantity=3,  # 1 * 3 spread units
            price=52.75,
        )

        put_leg_fill = self.create_leg_fill(
            instrument_id=self.put_option.id,
            client_order_id=ClientOrderId("O-20250725-080046-TEST-000-1-LEG-E1AQ5 P6440"),
            venue_order_id=VenueOrderId("213-LEG-1"),
            trade_id=TradeId("0000e1a7.6882c67b.02.01-1"),
            side=OrderSide.BUY,
            quantity=6,  # 2 * 3 spread units
            price=66.75,
        )

        # Act
        self.exec_engine.process(call_leg_fill)
        self.exec_engine.process(put_leg_fill)

        # Assert
        positions = self.cache.positions()
        assert len(positions) == 2

        # Find positions by instrument
        call_position = next(p for p in positions if p.instrument_id == self.call_option.id)
        put_position = next(p for p in positions if p.instrument_id == self.put_option.id)

        # Verify call position
        assert call_position.quantity == Quantity.from_int(3)
        assert call_position.side.name == "LONG"

        # Verify put position
        assert put_position.quantity == Quantity.from_int(6)
        assert put_position.side.name == "LONG"
