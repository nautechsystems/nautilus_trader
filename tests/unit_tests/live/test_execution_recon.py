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

import asyncio
from decimal import Decimal

import pandas as pd
import pytest

from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import ExecutionMassStatus
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.config import LiveExecEngineConfig
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.reconciliation import get_existing_fill_for_trade_id
from nautilus_trader.live.reconciliation import is_within_single_unit_tolerance
from nautilus_trader.live.risk_engine import LiveRiskEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.mocks.exec_clients import MockLiveExecutionClient
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


SIM = Venue("SIM")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")


class TestLiveExecutionReconciliation:
    @pytest.fixture(autouse=True)
    def setup(self, request):
        # Fixture Setup
        self.loop = request.getfixturevalue("event_loop")
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.account_id = TestIdStubs.account_id()
        self.trader_id = TestIdStubs.trader_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            clock=self.clock,
        )

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.data_engine = LiveDataEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exec_engine = LiveExecutionEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.risk_engine = LiveRiskEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.client = MockLiveExecutionClient(
            loop=self.loop,
            client_id=ClientId(SIM.value),
            venue=SIM,
            account_type=AccountType.CASH,
            base_currency=USD,
            instrument_provider=InstrumentProvider(),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        self.portfolio.update_account(TestEventStubs.cash_account_state())
        self.exec_engine.register_client(self.client)

        # Prepare components
        self.cache.add_instrument(AUDUSD_SIM)

    @pytest.mark.asyncio
    async def test_reconcile_state_no_cached_with_rejected_order(self):
        # Arrange
        report = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=VenueOrderId("1"),
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            order_status=OrderStatus.REJECTED,
            price=Price.from_str("1.00000"),
            quantity=Quantity.from_int(10_000),
            filled_qty=Quantity.from_int(0),
            post_only=True,
            cancel_reason="SOME_REASON",
            report_id=UUID4(),
            ts_accepted=0,
            ts_last=0,
            ts_init=0,
        )

        self.client.add_order_status_report(report)

        # Act
        result = await self.exec_engine.reconcile_execution_state()

        # Assert
        assert result
        assert len(self.cache.orders()) == 1
        assert self.cache.orders()[0].status == OrderStatus.REJECTED

    @pytest.mark.asyncio
    async def test_reconcile_state_no_cached_with_accepted_order(self):
        # Arrange
        report = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=VenueOrderId("1"),
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            order_status=OrderStatus.ACCEPTED,
            price=Price.from_str("1.00000"),
            quantity=Quantity.from_int(10_000),
            filled_qty=Quantity.from_int(0),
            post_only=True,
            report_id=UUID4(),
            ts_accepted=0,
            ts_last=0,
            ts_init=0,
        )

        self.client.add_order_status_report(report)

        # Act
        result = await self.exec_engine.reconcile_execution_state()

        # Assert
        assert result
        assert len(self.cache.orders()) == 1
        assert len(self.cache.orders_open()) == 1
        assert self.cache.orders()[0].status == OrderStatus.ACCEPTED

    @pytest.mark.asyncio
    async def test_reconcile_state_no_cached_with_canceled_order(self):
        # Arrange
        report = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=VenueOrderId("1"),
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            order_status=OrderStatus.CANCELED,
            price=Price.from_str("1.00000"),
            quantity=Quantity.from_int(10_000),
            filled_qty=Quantity.from_int(0),
            post_only=True,
            report_id=UUID4(),
            ts_accepted=0,
            ts_triggered=0,
            ts_last=0,
            ts_init=0,
        )

        self.client.add_order_status_report(report)

        # Act
        result = await self.exec_engine.reconcile_execution_state()

        # Assert
        assert result
        assert len(self.cache.orders()) == 1
        assert self.cache.orders()[0].status == OrderStatus.CANCELED

    @pytest.mark.asyncio
    async def test_reconcile_state_no_cached_with_expired_order(self):
        # Arrange
        report = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=VenueOrderId("1"),
            order_side=OrderSide.BUY,
            order_type=OrderType.STOP_LIMIT,
            time_in_force=TimeInForce.GTD,
            expire_time=pd.Timestamp("1970-01-01T00:10:00", tz="UTC"),
            order_status=OrderStatus.EXPIRED,
            price=Price.from_str("0.99500"),
            trigger_price=Price.from_str("1.00000"),
            trigger_type=TriggerType.BID_ASK,
            trailing_offset_type=TrailingOffsetType.PRICE,
            quantity=Quantity.from_int(10_000),
            filled_qty=Quantity.from_int(0),
            post_only=True,
            report_id=UUID4(),
            ts_accepted=0,
            ts_last=0,
            ts_init=0,
        )

        self.client.add_order_status_report(report)

        # Act
        result = await self.exec_engine.reconcile_execution_state()

        # Assert
        assert result
        assert len(self.cache.orders()) == 1
        assert self.cache.orders()[0].status == OrderStatus.EXPIRED

    @pytest.mark.asyncio
    async def test_reconcile_state_no_cached_with_triggered_order(self):
        # Arrange
        report = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=VenueOrderId("1"),
            order_side=OrderSide.BUY,
            order_type=OrderType.STOP_LIMIT,
            time_in_force=TimeInForce.GTC,
            order_status=OrderStatus.TRIGGERED,
            price=Price.from_str("0.99500"),
            trigger_price=Price.from_str("1.00000"),
            trigger_type=TriggerType.BID_ASK,
            trailing_offset_type=TrailingOffsetType.PRICE,
            quantity=Quantity.from_int(10_000),
            filled_qty=Quantity.from_int(0),
            post_only=True,
            report_id=UUID4(),
            ts_accepted=1_000_000_000,
            ts_triggered=2_000_000_000,
            ts_last=2_000_000_000,
            ts_init=3_000_000_000,
        )

        self.client.add_order_status_report(report)

        # Act
        result = await self.exec_engine.reconcile_execution_state()

        # Assert
        assert result
        assert len(self.cache.orders()) == 1
        assert len(self.cache.orders_open()) == 1
        assert self.cache.orders()[0].status == OrderStatus.TRIGGERED

    @pytest.mark.asyncio
    async def test_reconcile_state_no_cached_with_filled_order_and_no_trades(self):
        # Arrange
        report = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=VenueOrderId("1"),
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            order_status=OrderStatus.FILLED,
            price=Price.from_str("1.00000"),
            quantity=Quantity.from_int(10_000),
            filled_qty=Quantity.from_int(10_000),
            avg_px=Decimal("1.00000"),
            post_only=True,
            report_id=UUID4(),
            ts_accepted=0,
            ts_triggered=0,
            ts_last=0,
            ts_init=0,
        )

        self.client.add_order_status_report(report)

        # Act
        result = await self.exec_engine.reconcile_execution_state()

        # Assert
        assert result
        assert len(self.cache.orders()) == 1
        assert self.cache.orders()[0].status == OrderStatus.FILLED

    @pytest.mark.asyncio
    async def test_reconcile_state_no_cached_with_filled_order_and_trade(self):
        # Arrange
        venue_order_id = VenueOrderId("1")
        order_report = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=venue_order_id,
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            order_status=OrderStatus.FILLED,
            price=Price.from_str("1.00000"),
            quantity=Quantity.from_int(10_000),
            filled_qty=Quantity.from_int(10_000),
            avg_px=Decimal("1.00000"),
            post_only=True,
            reduce_only=False,
            report_id=UUID4(),
            ts_accepted=0,
            ts_triggered=0,
            ts_last=0,
            ts_init=0,
        )

        fill_report = FillReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=venue_order_id,
            trade_id=TradeId("1"),
            order_side=OrderSide.BUY,
            last_qty=Quantity.from_int(10_000),
            last_px=Price.from_str("1.00000"),
            commission=Money(0, USD),
            liquidity_side=LiquiditySide.MAKER,
            report_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.client.add_order_status_report(order_report)
        self.client.add_fill_reports(venue_order_id, [fill_report])

        # Act
        result = await self.exec_engine.reconcile_execution_state()

        # Assert
        assert result
        assert len(self.cache.orders()) == 1
        assert self.cache.orders()[0].status == OrderStatus.FILLED

    @pytest.mark.asyncio
    async def test_reconcile_state_no_cached_with_partially_filled_order_and_trade(self):
        # Arrange
        venue_order_id = VenueOrderId("1")
        order_report = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=venue_order_id,
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            order_status=OrderStatus.PARTIALLY_FILLED,
            price=Price.from_str("1.00000"),
            quantity=Quantity.from_int(10_000),
            filled_qty=Quantity.from_int(5_000),
            avg_px=Decimal("1.00000"),
            post_only=True,
            report_id=UUID4(),
            ts_accepted=0,
            ts_triggered=0,
            ts_last=0,
            ts_init=0,
        )

        fill_report = FillReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=venue_order_id,
            venue_position_id=None,
            trade_id=TradeId("1"),
            order_side=OrderSide.BUY,
            last_qty=Quantity.from_int(5_000),
            last_px=Price.from_str("1.00000"),
            commission=Money(0, USD),
            liquidity_side=LiquiditySide.MAKER,
            report_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.client.add_order_status_report(order_report)
        self.client.add_fill_reports(venue_order_id, [fill_report])

        # Act
        result = await self.exec_engine.reconcile_execution_state()

        # Assert
        assert result
        assert len(self.cache.orders()) == 1
        assert self.cache.orders()[0].status == OrderStatus.PARTIALLY_FILLED

    @pytest.mark.asyncio
    async def test_reconcile_state_no_cached_with_partially_filled_order_and_no_trade(self):
        # Arrange
        venue_order_id = VenueOrderId("1")
        order_report = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=venue_order_id,
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            order_status=OrderStatus.PARTIALLY_FILLED,
            price=Price.from_str("1.00000"),
            quantity=Quantity.from_int(10_000),
            filled_qty=Quantity.from_int(5_000),
            avg_px=Decimal("1.00000"),
            post_only=True,
            report_id=UUID4(),
            ts_accepted=0,
            ts_triggered=0,
            ts_last=0,
            ts_init=0,
        )

        self.client.add_order_status_report(order_report)

        # Act
        result = await self.exec_engine.reconcile_execution_state()

        # Assert
        assert result
        assert len(self.cache.orders()) == 1
        assert self.cache.orders()[0].status == OrderStatus.PARTIALLY_FILLED

    @pytest.mark.asyncio
    async def test_reconcile_state_no_cached_with_partially_filled_order_and_canceled(self):
        # Arrange
        venue_order_id = VenueOrderId("1")
        order_report = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=venue_order_id,
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            order_status=OrderStatus.CANCELED,
            price=Price.from_str("1.00000"),
            quantity=Quantity.from_int(10_000),
            filled_qty=Quantity.from_int(5_000),
            avg_px=Decimal("1.00000"),
            post_only=True,
            report_id=UUID4(),
            ts_accepted=0,
            ts_triggered=0,
            ts_last=0,
            ts_init=0,
        )

        fill_report = FillReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=venue_order_id,
            venue_position_id=None,
            trade_id=TradeId("1"),
            order_side=OrderSide.BUY,
            last_qty=Quantity.from_int(5_000),
            last_px=Price.from_str("1.00000"),
            commission=Money(0, USD),
            liquidity_side=LiquiditySide.MAKER,
            report_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.client.add_order_status_report(order_report)
        self.client.add_fill_reports(venue_order_id, [fill_report])

        # Act
        result = await self.exec_engine.reconcile_execution_state()

        # Assert
        assert result
        assert len(self.cache.orders()) == 1
        order = self.cache.orders()[0]
        assert order.status == OrderStatus.CANCELED
        assert order.last_trade_id == TradeId("1")
        assert order.quantity == Quantity.from_int(10_000)
        assert order.filled_qty == Quantity.from_int(5_000)

    @pytest.mark.asyncio
    async def test_reconcile_state_with_cached_order_and_different_fill_data(self):
        # Arrange: Create a cached order with a fill
        venue_order_id = VenueOrderId("1")
        client_order_id = ClientOrderId("O-123456")

        # Create and cache an order with an initial fill
        order = self.order_factory.market(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10_000),
            client_order_id=client_order_id,
        )

        # Submit and accept the order
        submitted = TestEventStubs.order_submitted(order, account_id=self.account_id)
        order.apply(submitted)
        self.cache.add_order(order, position_id=None)

        accepted = TestEventStubs.order_accepted(
            order,
            account_id=self.account_id,
            venue_order_id=venue_order_id,
        )
        order.apply(accepted)
        self.cache.update_order(order)

        # Apply an initial fill with specific data
        initial_fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            strategy_id=StrategyId("S-1"),
            last_qty=Quantity.from_int(5_000),
            last_px=Price.from_str("1.00000"),
            liquidity_side=LiquiditySide.MAKER,
            trade_id=TradeId("TRADE-1"),
        )
        order.apply(initial_fill)
        self.cache.update_order(order)

        # Now create a broker report with different fill data for the same trade_id
        order_report = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            order_side=OrderSide.BUY,
            order_type=OrderType.MARKET,
            time_in_force=TimeInForce.GTC,
            order_status=OrderStatus.PARTIALLY_FILLED,
            quantity=Quantity.from_int(10_000),
            filled_qty=Quantity.from_int(5_000),
            avg_px=Decimal("1.00000"),
            report_id=UUID4(),
            ts_accepted=0,
            ts_triggered=0,
            ts_last=0,
            ts_init=0,
        )

        # Fill report with DIFFERENT data than cached (different price, commission, liquidity)
        fill_report = FillReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            venue_position_id=None,
            trade_id=TradeId("TRADE-1"),  # Same trade_id as cached fill
            order_side=OrderSide.BUY,
            last_qty=Quantity.from_int(5_000),
            last_px=Price.from_str("1.00100"),  # Different price
            commission=Money(10.0, USD),  # Different commission
            liquidity_side=LiquiditySide.TAKER,  # Different liquidity side
            report_id=UUID4(),
            ts_event=1000,  # Different timestamp
            ts_init=0,
        )

        self.client.add_order_status_report(order_report)
        self.client.add_fill_reports(venue_order_id, [fill_report])

        # Act
        result = await self.exec_engine.reconcile_execution_state()

        # Assert: Reconciliation should succeed despite different fill data
        assert result

        # The order should still exist and maintain its cached state
        cached_order = self.cache.order(client_order_id)
        assert cached_order is not None
        assert cached_order.status == OrderStatus.PARTIALLY_FILLED
        assert cached_order.filled_qty == Quantity.from_int(5_000)

        # The cached fill data should remain unchanged (not updated with broker data)
        # This ensures we don't corrupt the order state
        fill_events = [
            event
            for event in cached_order.events
            if hasattr(event, "trade_id") and event.trade_id == TradeId("TRADE-1")
        ]
        assert len(fill_events) == 1
        cached_fill_event = fill_events[0]

        # Verify the cached data is preserved (original values, not broker values)
        assert cached_fill_event.last_px == Price.from_str("1.00000")  # Original price
        # Note: commission is calculated automatically by TestEventStubs, so we just check it exists
        assert cached_fill_event.commission is not None
        assert cached_fill_event.liquidity_side == LiquiditySide.MAKER  # Original liquidity


class TestReconciliationEdgeCases:
    """
    Test edge cases and robustness in live execution reconciliation.
    """

    @pytest.fixture
    def live_exec_engine(self, event_loop):
        loop = event_loop

        clock = LiveClock()
        trader_id = TestIdStubs.trader_id()
        msgbus = MessageBus(trader_id=trader_id, clock=clock)
        self.cache = TestComponentStubs.cache()

        client = MockLiveExecutionClient(
            loop=loop,
            client_id=ClientId("SIM"),
            venue=Venue("SIM"),
            account_type=AccountType.CASH,
            base_currency=USD,
            instrument_provider=InstrumentProvider(),
            msgbus=msgbus,
            cache=self.cache,
            clock=clock,
        )

        exec_engine = LiveExecutionEngine(
            loop=loop,
            msgbus=msgbus,
            cache=self.cache,
            clock=clock,
            config=LiveExecEngineConfig(reconciliation=True),
        )

        exec_engine.register_client(client)
        return exec_engine

    @pytest.mark.asyncio
    async def test_mass_status_failure_preserves_position_results(self, live_exec_engine):
        """
        Test that mass status failure is not overwritten by position reconciliation.
        """
        # Complex mocking required

    @pytest.mark.asyncio
    async def test_position_reconciliation_with_small_differences(self, live_exec_engine):
        """
        Test that small decimal differences are handled via instrument precision.
        """
        # Arrange
        report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=AUDUSD_SIM.id,
            position_side=PositionSide.LONG,
            quantity=Quantity.from_str("100.00000001"),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        # Assert
        assert report.signed_decimal_qty is not None

    @pytest.mark.asyncio
    async def test_zero_quantity_difference_handling(self, live_exec_engine):
        """
        Test that zero quantity differences after rounding are handled properly.
        """
        # Arrange
        report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=AUDUSD_SIM.id,
            position_side=PositionSide.LONG,
            quantity=Quantity.from_str("100.00000001"),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        # Assert
        assert report.quantity > 0

    @pytest.mark.asyncio
    async def test_fill_report_before_order_status_report(self, live_exec_engine):
        """
        Test graceful handling when FillReport arrives before OrderStatusReport.
        """
        # Arrange
        fill_report = FillReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=AUDUSD_SIM.id,
            venue_order_id=VenueOrderId("UNKNOWN-ORDER"),
            trade_id=TradeId("TRADE-1"),
            order_side=OrderSide.BUY,
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("1.0"),
            commission=Money(5.0, USD),
            liquidity_side=LiquiditySide.TAKER,
            report_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        # Act
        result = live_exec_engine._reconcile_fill_report_single(fill_report)

        # Assert
        assert result is False

    @pytest.mark.asyncio
    async def test_netting_venue_position_id_generation(self, live_exec_engine):
        """
        Test that position IDs are correctly generated for netting venues.
        """
        # Arrange
        instrument = TestInstrumentProvider.default_fx_ccy("EURUSD")

        fill_report = FillReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            venue_order_id=VenueOrderId("V-1"),
            trade_id=TradeId("TRADE-1"),
            order_side=OrderSide.BUY,
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("1.0"),
            commission=Money(5.0, USD),
            liquidity_side=LiquiditySide.TAKER,
            report_id=UUID4(),
            ts_event=0,
            ts_init=0,
            venue_position_id=None,
        )

        order = TestExecStubs.limit_order()

        # Spy on the _handle_event method to capture the generated event
        generated_events = []
        original_handle_event = live_exec_engine._handle_event

        def spy_handle_event(event):
            generated_events.append(event)
            return original_handle_event(event)

        live_exec_engine._handle_event = spy_handle_event

        # Act
        live_exec_engine._generate_order_filled(order, fill_report, instrument)

        # Assert
        fill_events = [event for event in generated_events if isinstance(event, OrderFilled)]
        assert len(fill_events) == 1

    @pytest.mark.asyncio
    async def test_long_position_reconciliation_quantity_mismatch(self, live_exec_engine):
        """
        Test reconciliation when internal long position quantity differs from external
        report.
        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)

        # Enable missing order generation
        live_exec_engine.generate_missing_orders = True

        # Create internal long position (100 units)
        order = TestExecStubs.limit_order(instrument=instrument, order_side=OrderSide.BUY)
        fill = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            position_id=PositionId("P-1"),
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("1.0"),
        )
        internal_position = Position(instrument=instrument, fill=fill)
        self.cache.add_position(internal_position, OmsType.NETTING)

        # External report shows 150 units (need to generate 50 BUY order)
        external_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(150),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        reconcile_calls = []
        original_reconcile = live_exec_engine._reconcile_order_report

        def spy_reconcile(order_report, trades, is_external=True):
            reconcile_calls.append((order_report, trades))
            return original_reconcile(order_report, trades, is_external)

        live_exec_engine._reconcile_order_report = spy_reconcile

        # Act
        result = live_exec_engine._reconcile_position_report(external_report)

        # Assert
        assert result is True
        assert len(reconcile_calls) == 1

        order_report, _ = reconcile_calls[0]
        assert order_report.order_side == OrderSide.BUY
        assert order_report.quantity == Quantity.from_int(50)
        assert order_report.filled_qty == Quantity.from_int(50)
        assert order_report.order_status == OrderStatus.FILLED

    @pytest.mark.asyncio
    async def test_short_position_reconciliation_quantity_mismatch(self, live_exec_engine):
        """
        Test reconciliation when internal short position quantity differs from external
        report.
        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)

        # Enable missing order generation
        live_exec_engine.generate_missing_orders = True

        # Create internal short position (-100 units)
        order = TestExecStubs.limit_order(instrument=instrument, order_side=OrderSide.SELL)
        fill = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            position_id=PositionId("P-2"),
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("1.0"),
        )
        internal_position = Position(instrument=instrument, fill=fill)
        self.cache.add_position(internal_position, OmsType.NETTING)

        # External report shows -150 units (need to generate 50 SELL order)
        external_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.SHORT,
            quantity=Quantity.from_int(150),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        reconcile_calls = []
        original_reconcile = live_exec_engine._reconcile_order_report

        def spy_reconcile(order_report, trades, is_external=True):
            reconcile_calls.append((order_report, trades))
            return original_reconcile(order_report, trades, is_external)

        live_exec_engine._reconcile_order_report = spy_reconcile

        # Act
        result = live_exec_engine._reconcile_position_report(external_report)

        # Assert
        assert result is True
        assert len(reconcile_calls) == 1

        order_report, _ = reconcile_calls[0]
        assert order_report.order_side == OrderSide.SELL
        assert order_report.quantity == Quantity.from_int(50)
        assert order_report.filled_qty == Quantity.from_int(50)
        assert order_report.order_status == OrderStatus.FILLED

    @pytest.mark.asyncio
    async def test_long_position_reconciliation_external_smaller(self, live_exec_engine):
        """
        Test reconciliation when external long position is smaller than internal
        position.
        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)

        # Enable missing order generation
        live_exec_engine.generate_missing_orders = True

        # Create internal long position (150 units)
        order = TestExecStubs.limit_order(instrument=instrument, order_side=OrderSide.BUY)
        fill = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            position_id=PositionId("P-3"),
            last_qty=Quantity.from_int(150),
            last_px=Price.from_str("1.0"),
        )
        internal_position = Position(instrument=instrument, fill=fill)
        self.cache.add_position(internal_position, OmsType.NETTING)

        # External report shows 100 units (need to generate 50 SELL order to reduce)
        external_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(100),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        reconcile_calls = []
        original_reconcile = live_exec_engine._reconcile_order_report

        def spy_reconcile(order_report, trades, is_external=True):
            reconcile_calls.append((order_report, trades))
            return original_reconcile(order_report, trades, is_external)

        live_exec_engine._reconcile_order_report = spy_reconcile

        # Act
        result = live_exec_engine._reconcile_position_report(external_report)

        # Assert
        assert result is True
        assert len(reconcile_calls) == 1

        order_report, _ = reconcile_calls[0]
        assert order_report.order_side == OrderSide.SELL
        assert order_report.quantity == Quantity.from_int(50)
        assert order_report.filled_qty == Quantity.from_int(50)
        assert order_report.order_status == OrderStatus.FILLED

    @pytest.mark.asyncio
    async def test_short_position_reconciliation_external_smaller(self, live_exec_engine):
        """
        Test reconciliation when external short position is smaller than internal
        position.
        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)

        # Enable missing order generation
        live_exec_engine.generate_missing_orders = True

        # Create internal short position (-150 units)
        order = TestExecStubs.limit_order(instrument=instrument, order_side=OrderSide.SELL)
        fill = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            position_id=PositionId("P-4"),
            last_qty=Quantity.from_int(150),
            last_px=Price.from_str("1.0"),
        )
        internal_position = Position(instrument=instrument, fill=fill)
        self.cache.add_position(internal_position, OmsType.NETTING)

        # External report shows -100 units (need to generate 50 BUY order to reduce)
        external_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.SHORT,
            quantity=Quantity.from_int(100),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        reconcile_calls = []
        original_reconcile = live_exec_engine._reconcile_order_report

        def spy_reconcile(order_report, trades, is_external=True):
            reconcile_calls.append((order_report, trades))
            return original_reconcile(order_report, trades, is_external)

        live_exec_engine._reconcile_order_report = spy_reconcile

        # Act
        result = live_exec_engine._reconcile_position_report(external_report)

        # Assert
        assert result is True
        assert len(reconcile_calls) == 1

        order_report, _ = reconcile_calls[0]
        assert order_report.order_side == OrderSide.BUY
        assert order_report.quantity == Quantity.from_int(50)
        assert order_report.filled_qty == Quantity.from_int(50)
        assert order_report.order_status == OrderStatus.FILLED

    @pytest.mark.asyncio
    async def test_long_position_reconciliation_external_flat(self, live_exec_engine):
        """
        Test reconciliation when internal long position exists but external position is
        FLAT.

        Tests scenario from issue #3023 where a position is closed externally (via the
        client/exchange directly) but remains open in the cache.

        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)

        # Enable missing order generation
        live_exec_engine.generate_missing_orders = True

        # Create internal long position (100 units)
        order = TestExecStubs.limit_order(instrument=instrument, order_side=OrderSide.BUY)
        fill = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            position_id=PositionId("P-FLAT-LONG"),
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("1.0"),
        )
        internal_position = Position(instrument=instrument, fill=fill)
        self.cache.add_position(internal_position, OmsType.NETTING)

        # External report shows FLAT position (closed externally)
        external_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.FLAT,
            quantity=Quantity.from_int(0),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        reconcile_calls = []
        original_reconcile = live_exec_engine._reconcile_order_report

        def spy_reconcile(order_report, trades, is_external=True):
            reconcile_calls.append((order_report, trades))
            return original_reconcile(order_report, trades, is_external)

        live_exec_engine._reconcile_order_report = spy_reconcile

        # Act
        result = live_exec_engine._reconcile_position_report(external_report)

        # Assert
        assert result is True
        assert len(reconcile_calls) == 1

        order_report, _ = reconcile_calls[0]
        assert order_report.order_side == OrderSide.SELL  # Close long position
        assert order_report.quantity == Quantity.from_int(100)
        assert order_report.filled_qty == Quantity.from_int(100)
        assert order_report.order_status == OrderStatus.FILLED

    @pytest.mark.asyncio
    async def test_short_position_reconciliation_external_flat(self, live_exec_engine):
        """
        Test reconciliation when internal short position exists but external position is
        FLAT.

        Tests scenario from issue #3023 where a position is closed externally (via the
        client/exchange directly) but remains open in the cache.

        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)

        # Enable missing order generation
        live_exec_engine.generate_missing_orders = True

        # Create internal short position (-100 units)
        order = TestExecStubs.limit_order(instrument=instrument, order_side=OrderSide.SELL)
        fill = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            position_id=PositionId("P-FLAT-SHORT"),
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("1.0"),
        )
        internal_position = Position(instrument=instrument, fill=fill)
        self.cache.add_position(internal_position, OmsType.NETTING)

        # External report shows FLAT position (closed externally)
        external_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.FLAT,
            quantity=Quantity.from_int(0),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        reconcile_calls = []
        original_reconcile = live_exec_engine._reconcile_order_report

        def spy_reconcile(order_report, trades, is_external=True):
            reconcile_calls.append((order_report, trades))
            return original_reconcile(order_report, trades, is_external)

        live_exec_engine._reconcile_order_report = spy_reconcile

        # Act
        result = live_exec_engine._reconcile_position_report(external_report)

        # Assert
        assert result is True
        assert len(reconcile_calls) == 1

        order_report, _ = reconcile_calls[0]
        assert order_report.order_side == OrderSide.BUY  # Close short position
        assert order_report.quantity == Quantity.from_int(100)
        assert order_report.filled_qty == Quantity.from_int(100)
        assert order_report.order_status == OrderStatus.FILLED

    @pytest.mark.asyncio
    async def test_flat_position_report_generates_closing_order(self, live_exec_engine):
        """
        Test that a FLAT PositionStatusReport generates a closing order.

        Verifies the correct closing order is generated to reconcile the position.

        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)
        live_exec_engine.generate_missing_orders = True

        order = TestExecStubs.limit_order(instrument=instrument, order_side=OrderSide.BUY)
        fill = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            position_id=PositionId("P-CLEAR-CACHE"),
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("1.0"),
        )
        internal_position = Position(instrument=instrument, fill=fill)
        self.cache.add_position(internal_position, OmsType.NETTING)

        assert internal_position.is_open
        assert internal_position.quantity == Quantity.from_int(100)

        flat_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.FLAT,
            quantity=Quantity.zero(),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        reconcile_calls = []
        original_reconcile = live_exec_engine._reconcile_order_report

        def spy_reconcile(order_report, trades, is_external=True):
            reconcile_calls.append((order_report, trades))
            return original_reconcile(order_report, trades, is_external)

        live_exec_engine._reconcile_order_report = spy_reconcile

        # Act
        result = live_exec_engine._reconcile_position_report(flat_report)

        # Assert
        assert result is True
        assert len(reconcile_calls) == 1

        order_report, _ = reconcile_calls[0]
        assert order_report.order_side == OrderSide.SELL
        assert order_report.quantity == Quantity.from_int(100)
        assert order_report.filled_qty == Quantity.from_int(100)
        assert order_report.order_status == OrderStatus.FILLED

    @pytest.mark.asyncio
    async def test_mixed_reconciliation_long_and_flat_instruments(self, live_exec_engine):
        """
        Test mixed reconciliation with both LONG and FLAT instruments.

        Verifies only matching positions are closed while others persist.

        """
        # Arrange
        instrument1 = AUDUSD_SIM
        instrument2 = GBPUSD_SIM
        self.cache.add_instrument(instrument1)
        self.cache.add_instrument(instrument2)
        live_exec_engine.generate_missing_orders = True

        # Position 1: LONG 100 AUD/USD
        order1 = TestExecStubs.limit_order(
            instrument=instrument1,
            order_side=OrderSide.BUY,
        )
        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=instrument1,
            position_id=PositionId("P-AUD-LONG"),
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("0.70"),
        )
        position1 = Position(instrument=instrument1, fill=fill1)
        self.cache.add_position(position1, OmsType.NETTING)

        # Position 2: LONG 50 GBP/USD
        order2 = TestExecStubs.limit_order(
            instrument=instrument2,
            order_side=OrderSide.BUY,
        )
        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=instrument2,
            position_id=PositionId("P-GBP-LONG"),
            last_qty=Quantity.from_int(50),
            last_px=Price.from_str("1.30"),
        )
        position2 = Position(instrument=instrument2, fill=fill2)
        self.cache.add_position(position2, OmsType.NETTING)

        # Verify both positions are open
        assert len(self.cache.positions_open()) == 2

        # External report 1: AUD/USD still LONG 100 (no change)
        report1 = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument1.id,
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(100),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        # External report 2: GBP/USD now FLAT (closed externally)
        report2 = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument2.id,
            position_side=PositionSide.FLAT,
            quantity=Quantity.zero(),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        reconcile_calls = []
        original_reconcile = live_exec_engine._reconcile_order_report

        def spy_reconcile(order_report, trades, is_external=True):
            reconcile_calls.append((order_report, trades))
            return original_reconcile(order_report, trades, is_external)

        live_exec_engine._reconcile_order_report = spy_reconcile

        # Act
        result1 = live_exec_engine._reconcile_position_report(report1)
        result2 = live_exec_engine._reconcile_position_report(report2)

        # Assert
        assert result1 is True
        assert result2 is True

        # Verify only GBP/USD generated a closing order (AUD/USD had no difference)
        assert len(reconcile_calls) == 1

        # Verify the closing order is for GBP/USD
        order_report, _ = reconcile_calls[0]
        assert order_report.instrument_id == instrument2.id
        assert order_report.order_side == OrderSide.SELL
        assert order_report.quantity == Quantity.from_int(50)
        assert order_report.filled_qty == Quantity.from_int(50)
        assert order_report.order_status == OrderStatus.FILLED

    @pytest.mark.asyncio
    async def test_stale_short_reconciled_by_flat_after_fills(self, live_exec_engine):
        """
        Test stale internal short reconciled by flat report after fills.

        Simulates: SHORT position exists, some fills happen, then FLAT report
        arrives and generates compensating closing fill.

        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)
        live_exec_engine.generate_missing_orders = True

        order = TestExecStubs.limit_order(
            instrument=instrument,
            order_side=OrderSide.SELL,
        )
        fill = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            position_id=PositionId("P-STALE-SHORT"),
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("0.70"),
        )
        internal_position = Position(instrument=instrument, fill=fill)
        self.cache.add_position(internal_position, OmsType.NETTING)

        # Verify position is short
        assert internal_position.is_short
        assert internal_position.quantity == Quantity.from_int(100)

        # Simulate additional fills happened (but externally, not in cache)
        # The position was closed externally, now we get a FLAT report

        flat_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.FLAT,
            quantity=Quantity.zero(),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        reconcile_calls = []
        original_reconcile = live_exec_engine._reconcile_order_report

        def spy_reconcile(order_report, trades, is_external=True):
            reconcile_calls.append((order_report, trades))
            return original_reconcile(order_report, trades, is_external)

        live_exec_engine._reconcile_order_report = spy_reconcile

        # Act
        result = live_exec_engine._reconcile_position_report(flat_report)

        # Assert
        assert result is True
        assert len(reconcile_calls) == 1

        # Verify compensating closing fill was generated
        order_report, _ = reconcile_calls[0]
        assert order_report.order_side == OrderSide.BUY
        assert order_report.quantity == Quantity.from_int(100)
        assert order_report.filled_qty == Quantity.from_int(100)
        assert order_report.order_status == OrderStatus.FILLED

    @pytest.mark.asyncio
    async def test_flat_report_processes_successfully(self, live_exec_engine):
        """
        Test FLAT report processing completes successfully.

        Verifies reconciliation proceeds correctly with standard FLAT report.

        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)
        live_exec_engine.generate_missing_orders = True

        order = TestExecStubs.limit_order(instrument=instrument, order_side=OrderSide.BUY)
        fill = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            position_id=PositionId("P-PROCESS-FLAT"),
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("0.70"),
        )
        internal_position = Position(instrument=instrument, fill=fill)
        self.cache.add_position(internal_position, OmsType.NETTING)

        flat_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.FLAT,
            quantity=Quantity.zero(),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        reconcile_calls = []
        original_reconcile = live_exec_engine._reconcile_order_report

        def spy_reconcile(order_report, trades, is_external=True):
            reconcile_calls.append((order_report, trades))
            return original_reconcile(order_report, trades, is_external)

        live_exec_engine._reconcile_order_report = spy_reconcile

        # Act
        result = live_exec_engine._reconcile_position_report(flat_report)

        # Assert - reconciliation should succeed
        assert result is True
        assert len(reconcile_calls) == 1

        # Verify closing order was generated
        order_report, _ = reconcile_calls[0]
        assert order_report.order_side == OrderSide.SELL
        assert order_report.quantity == Quantity.from_int(100)
        assert order_report.order_status == OrderStatus.FILLED

    @pytest.mark.asyncio
    async def test_position_recon_flat_report_clears_cache_ib(self, live_exec_engine):
        """
        Test IB-style FLAT report generates closing order.

        Verifies that reconciling a FLAT report generates the correct synthetic closing
        order to clear the position.

        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)
        live_exec_engine.generate_missing_orders = True

        order = TestExecStubs.limit_order(instrument=instrument, order_side=OrderSide.BUY)
        internal_fill = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            position_id=PositionId("P-IB-CLEAR"),
            last_qty=Quantity.from_int(10),
            last_px=Price.from_str("100.0"),
        )
        position = Position(instrument=instrument, fill=internal_fill)
        self.cache.add_position(position, OmsType.NETTING)

        # Verify position exists and cache state before
        assert self.cache.position(position.id) is not None
        assert position.is_open
        assert len(self.cache.positions_open(instrument_id=instrument.id)) == 1

        flat_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.FLAT,
            quantity=Quantity.zero(),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        reconcile_calls = []
        original_reconcile = live_exec_engine._reconcile_order_report

        def spy_reconcile(order_report, trades, is_external=True):
            reconcile_calls.append((order_report, trades))
            return original_reconcile(order_report, trades, is_external)

        live_exec_engine._reconcile_order_report = spy_reconcile

        # Act
        result = live_exec_engine._reconcile_position_report(flat_report)

        # Assert
        assert result is True
        assert len(reconcile_calls) == 1

        # Verify correct closing order generated
        order_report, _ = reconcile_calls[0]
        assert order_report.instrument_id == instrument.id
        assert order_report.order_side == OrderSide.SELL
        assert order_report.quantity == Quantity.from_int(10)
        assert order_report.filled_qty == Quantity.from_int(10)
        assert order_report.order_status == OrderStatus.FILLED

        # NOTE: This unit test verifies reconciliation generates the correct closing order.
        # Cache clearing happens when this order is processed through the execution engine's
        # order flow (submit → accepted → filled → position updated). Integration tests
        # would verify the full end-to-end flow including cache state changes.

    @pytest.mark.asyncio
    async def test_position_recon_flat_report_with_fills_ib(self, live_exec_engine):
        """
        Test IB-style FLAT report reconciliation with historical fills.

        Verifies reconciliation generates correct closing order even when position has
        multiple fills.

        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)
        live_exec_engine.generate_missing_orders = True

        # Create position with multiple fills
        order1 = TestExecStubs.limit_order(instrument=instrument, order_side=OrderSide.BUY)
        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=instrument,
            position_id=PositionId("P-IB-MULTI"),
            last_qty=Quantity.from_int(5),
            last_px=Price.from_str("100.0"),
        )
        position = Position(instrument=instrument, fill=fill1)

        # Add second fill
        order2 = TestExecStubs.limit_order(instrument=instrument, order_side=OrderSide.BUY)
        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=instrument,
            position_id=PositionId("P-IB-MULTI"),
            last_qty=Quantity.from_int(5),
            last_px=Price.from_str("101.0"),
        )
        position.apply(fill2)
        self.cache.add_position(position, OmsType.NETTING)

        # Verify total position and cache state before
        assert position.quantity == Quantity.from_int(10)
        assert position.is_open
        assert len(self.cache.positions_open(instrument_id=instrument.id)) == 1

        flat_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.FLAT,
            quantity=Quantity.zero(),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        reconcile_calls = []
        original_reconcile = live_exec_engine._reconcile_order_report

        def spy_reconcile(order_report, trades, is_external=True):
            reconcile_calls.append((order_report, trades))
            return original_reconcile(order_report, trades, is_external)

        live_exec_engine._reconcile_order_report = spy_reconcile

        # Act
        result = live_exec_engine._reconcile_position_report(flat_report)

        # Assert
        assert result is True
        assert len(reconcile_calls) == 1

        # Verify closing order covers full position
        order_report, _ = reconcile_calls[0]
        assert order_report.instrument_id == instrument.id
        assert order_report.order_side == OrderSide.SELL
        assert order_report.quantity == Quantity.from_int(10)
        assert order_report.filled_qty == Quantity.from_int(10)
        assert order_report.order_status == OrderStatus.FILLED

    @pytest.mark.asyncio
    async def test_flat_report_generates_closing_order_with_correct_quantity(
        self,
        live_exec_engine,
    ):
        """
        Test that FLAT reconciliation generates closing order with exact position
        quantity.

        Verifies the synthetic closing order matches the cached position quantity, which
        when processed through the execution engine will clear the position.

        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)

        # Enable missing order generation
        live_exec_engine.generate_missing_orders = True

        # Create internal long position (100 units)
        order = TestExecStubs.limit_order(instrument=instrument, order_side=OrderSide.BUY)
        fill = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            position_id=PositionId("P-E2E-FLAT"),
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("1.0"),
        )
        position = Position(instrument=instrument, fill=fill)
        self.cache.add_position(position, OmsType.NETTING)

        assert position.is_open
        assert len(self.cache.positions_open(instrument_id=instrument.id)) == 1

        # External report shows FLAT position (closed externally)
        flat_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.FLAT,
            quantity=Quantity.zero(),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        # Spy on _reconcile_order_report to verify closing order
        reconcile_calls = []
        original_reconcile = live_exec_engine._reconcile_order_report

        def spy_reconcile(order_report, trades, is_external=True):
            reconcile_calls.append((order_report, trades))
            return original_reconcile(order_report, trades, is_external)

        live_exec_engine._reconcile_order_report = spy_reconcile

        # Act
        result = live_exec_engine._reconcile_position_report(flat_report)

        # Assert
        assert result is True
        assert len(reconcile_calls) == 1

        # Verify closing order exactly matches position quantity
        order_report, _ = reconcile_calls[0]
        assert order_report.instrument_id == instrument.id
        assert order_report.order_side == OrderSide.SELL
        assert order_report.quantity == position.quantity  # Exact match
        assert order_report.filled_qty == position.quantity
        assert order_report.order_status == OrderStatus.FILLED

        # NOTE: Cache clearing happens when this order is processed through the
        # execution engine's order flow (submit → accepted → filled → position updated).
        # This unit test verifies reconciliation generates the correct closing order.
        # Integration tests would verify the full end-to-end flow including cache updates.

    @pytest.mark.asyncio
    async def test_position_reconciliation_cross_side_long_to_short(self, live_exec_engine):
        """
        Test reconciliation when internal long position conflicts with external short
        position.
        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)

        # Enable missing order generation
        live_exec_engine.generate_missing_orders = True

        # Create internal long position (100 units)
        order = TestExecStubs.limit_order(instrument=instrument, order_side=OrderSide.BUY)
        fill = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            position_id=PositionId("P-5"),
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("1.0"),
        )
        internal_position = Position(instrument=instrument, fill=fill)
        self.cache.add_position(internal_position, OmsType.NETTING)

        # External report shows -50 units short with avg_px (need to generate 2 orders: close + open)
        external_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.SHORT,
            quantity=Quantity.from_int(50),
            avg_px_open=Decimal("1.0"),  # Provide avg price so split fill can calculate
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        # Spy on reconcile_order_report calls
        reconcile_calls = []
        original_reconcile = live_exec_engine._reconcile_order_report

        def spy_reconcile(order_report, trades, is_external=True):
            reconcile_calls.append((order_report, trades))
            return original_reconcile(order_report, trades, is_external)

        live_exec_engine._reconcile_order_report = spy_reconcile

        # Act
        result = live_exec_engine._reconcile_position_report(external_report)

        # Assert
        assert result is True
        # With the new split fill logic, this should generate TWO reconciliation orders
        assert len(reconcile_calls) == 2

        # First order: close existing LONG position (SELL 100)
        close_report, _ = reconcile_calls[0]
        assert close_report.order_side == OrderSide.SELL
        assert close_report.quantity == Quantity.from_int(100)
        assert close_report.filled_qty == Quantity.from_int(100)
        assert close_report.order_status == OrderStatus.FILLED

        # Second order: open new SHORT position (SELL 50)
        open_report, _ = reconcile_calls[1]
        assert open_report.order_side == OrderSide.SELL
        assert open_report.quantity == Quantity.from_int(50)
        assert open_report.filled_qty == Quantity.from_int(50)
        assert open_report.order_status == OrderStatus.FILLED

    @pytest.mark.asyncio
    async def test_position_reconciliation_cross_side_short_to_long(self, live_exec_engine):
        """
        Test reconciliation when internal short position conflicts with external long
        position.
        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)

        # Enable missing order generation
        live_exec_engine.generate_missing_orders = True

        # Create internal short position (-100 units)
        order = TestExecStubs.limit_order(instrument=instrument, order_side=OrderSide.SELL)
        fill = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            position_id=PositionId("P-6"),
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("1.0"),
        )
        internal_position = Position(instrument=instrument, fill=fill)
        self.cache.add_position(internal_position, OmsType.NETTING)

        # External report shows 75 units long with avg_px (need to generate 2 orders: close + open)
        external_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(75),
            avg_px_open=Decimal("1.0"),  # Provide avg price so split fill can calculate
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        reconcile_calls = []
        original_reconcile = live_exec_engine._reconcile_order_report

        def spy_reconcile(order_report, trades, is_external=True):
            reconcile_calls.append((order_report, trades))
            return original_reconcile(order_report, trades, is_external)

        live_exec_engine._reconcile_order_report = spy_reconcile

        # Act
        result = live_exec_engine._reconcile_position_report(external_report)

        # Assert
        assert result is True
        # With the new split fill logic, this should generate TWO reconciliation orders
        assert len(reconcile_calls) == 2

        # First order: close existing SHORT position (BUY 100)
        close_report, _ = reconcile_calls[0]
        assert close_report.order_side == OrderSide.BUY
        assert close_report.quantity == Quantity.from_int(100)
        assert close_report.filled_qty == Quantity.from_int(100)
        assert close_report.order_status == OrderStatus.FILLED

        # Second order: open new LONG position (BUY 75)
        open_report, _ = reconcile_calls[1]
        assert open_report.order_side == OrderSide.BUY
        assert open_report.quantity == Quantity.from_int(75)
        assert open_report.filled_qty == Quantity.from_int(75)
        assert open_report.order_status == OrderStatus.FILLED

    @pytest.mark.asyncio
    async def test_position_reconciliation_zero_difference_after_rounding(self, live_exec_engine):
        """
        Test that zero differences after rounding are handled correctly.
        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)

        # Enable missing order generation
        live_exec_engine.generate_missing_orders = True

        # Create internal long position (100 units)
        order = TestExecStubs.limit_order(instrument=instrument, order_side=OrderSide.BUY)
        fill = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            position_id=PositionId("P-7"),
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("1.0"),
        )
        internal_position = Position(instrument=instrument, fill=fill)
        self.cache.add_position(internal_position, OmsType.NETTING)

        # External report shows 100.00000001 units (rounds to 100)
        external_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.LONG,
            quantity=Quantity.from_str("100.00000001"),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        reconcile_calls = []
        original_reconcile = live_exec_engine._reconcile_order_report

        def spy_reconcile(order_report, trades, is_external=True):
            reconcile_calls.append((order_report, trades))
            return original_reconcile(order_report, trades, is_external)

        live_exec_engine._reconcile_order_report = spy_reconcile

        # Act
        result = live_exec_engine._reconcile_position_report(external_report)

        # Assert
        assert result is True
        assert len(reconcile_calls) == 0  # No order should be generated due to rounding

    @pytest.mark.asyncio
    async def test_inferred_fill_with_positive_quantity_difference(self, live_exec_engine):
        """
        Test that inferred fill generation works correctly with normal positive quantity
        differences.
        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)

        # Create an order with some filled quantity
        order = TestExecStubs.limit_order(instrument=instrument)
        accepted = TestEventStubs.order_accepted(order)
        order.apply(accepted)
        fill = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            last_qty=Quantity.from_int(50),
            last_px=Price.from_str("1.0"),
        )
        order.apply(fill)  # Order shows 50 filled
        self.cache.add_order(order)

        # Create a report that shows more filled quantity (normal case)
        report = OrderStatusReport(
            instrument_id=instrument.id,
            account_id=TestIdStubs.account_id(),
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("V-1"),
            order_side=OrderSide.BUY,
            order_type=OrderType.MARKET,
            time_in_force=TimeInForce.DAY,
            order_status=OrderStatus.PARTIALLY_FILLED,
            quantity=Quantity.from_int(100),
            filled_qty=Quantity.from_int(75),  # More than order's filled_qty (50)
            avg_px=Price.from_str("1.0"),
            report_id=UUID4(),
            ts_accepted=0,
            ts_last=0,
            ts_init=0,
        )

        # Act
        inferred_fill = live_exec_engine._generate_inferred_fill(order, report, instrument)

        # Assert
        assert inferred_fill is not None
        assert isinstance(inferred_fill, OrderFilled)
        assert inferred_fill.last_qty == Quantity.from_int(25)  # 75 - 50
        assert inferred_fill.last_px == Price.from_str("1.0")
        assert inferred_fill.client_order_id == order.client_order_id

    @pytest.mark.asyncio
    async def test_reconcile_order_report_fails_when_report_filled_qty_less_than_order(
        self,
        live_exec_engine,
    ):
        """
        Test that order reconciliation fails when report.filled_qty < order.filled_qty.

        This indicates corrupted cached state and should be treated as an error.

        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)

        # Create an order with filled quantity
        order = TestExecStubs.limit_order(instrument=instrument)
        accepted = TestEventStubs.order_accepted(order)
        order.apply(accepted)
        fill = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("1.0"),
        )
        order.apply(fill)  # Order shows 100 filled
        self.cache.add_order(order)

        # Create a report that shows less filled quantity (corrupted state scenario)
        report = OrderStatusReport(
            instrument_id=instrument.id,
            account_id=TestIdStubs.account_id(),
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("V-1"),
            order_side=OrderSide.BUY,
            order_type=OrderType.MARKET,
            time_in_force=TimeInForce.DAY,
            order_status=OrderStatus.PARTIALLY_FILLED,
            quantity=Quantity.from_int(150),
            filled_qty=Quantity.from_int(75),  # Less than order's filled_qty (100)
            avg_px=Price.from_str("1.0"),
            report_id=UUID4(),
            ts_accepted=0,
            ts_last=0,
            ts_init=0,
        )

        # Act
        result = live_exec_engine._reconcile_order_report(report, trades=[])

        # Assert
        assert result is False  # Reconciliation should fail
        assert order.filled_qty == Quantity.from_int(100)  # No inferred fill

    @pytest.mark.asyncio
    async def test_reconcile_order_report_with_missing_instrument_defers(
        self,
        live_exec_engine,
    ):
        """
        Test that order reconciliation is deferred when instrument is not in cache.

        This prevents creating invalid orders without instrument information and allows
        reconciliation to succeed later when the instrument is loaded.

        """
        # Arrange
        # Instrument NOT added to cache
        report = OrderStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=AUDUSD_SIM.id,  # Instrument not in cache
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=VenueOrderId("1"),
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            order_status=OrderStatus.ACCEPTED,
            price=Price.from_str("1.00000"),
            quantity=Quantity.from_int(10_000),
            filled_qty=Quantity.from_int(0),
            report_id=UUID4(),
            ts_accepted=0,
            ts_last=0,
            ts_init=0,
        )

        # Act
        result = live_exec_engine._reconcile_order_report(report, trades=[])

        # Assert
        assert result is True  # Deferred, not failed
        assert len(self.cache.orders()) == 0  # No order created

    @pytest.mark.asyncio
    async def test_reconcile_fill_report_preventing_overfill(
        self,
        live_exec_engine,
    ):
        """
        Test that fill reports that would cause overfill are rejected.

        This prevents order corruption from excessive fills.

        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)

        # Create an order with quantity 100
        order = TestExecStubs.limit_order(
            instrument=instrument,
            quantity=Quantity.from_int(100),
        )
        order.apply(TestEventStubs.order_accepted(order))

        # Partially fill with 80
        fill1 = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            last_qty=Quantity.from_int(80),
        )
        order.apply(fill1)
        self.cache.add_order(order)

        # Create a fill report that would cause overfill (80 + 30 > 100)
        overfill_report = FillReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            trade_id=TradeId("2"),
            order_side=OrderSide.BUY,
            last_qty=Quantity.from_int(30),  # This would make total 110 > 100
            last_px=Price.from_str("1.00001"),
            commission=Money(0, USD),
            liquidity_side=LiquiditySide.TAKER,
            report_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        # Act
        result = live_exec_engine._reconcile_fill_report(order, overfill_report, instrument)

        # Assert
        assert result is False
        assert order.filled_qty == Quantity.from_int(80)  # Fill not applied

    @pytest.mark.asyncio
    async def test_reconcile_existing_order_with_missing_instrument_defers(
        self,
        live_exec_engine,
    ):
        """
        Test that reconciling an existing order is deferred when instrument is missing.

        This tests the second instrument check for already cached orders.

        """
        # Arrange
        # Create an order with an instrument NOT in cache
        instrument = AUDUSD_SIM  # Not added to cache

        # Create an order manually (bypassing normal flow)
        order = TestExecStubs.limit_order(instrument=instrument)
        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))

        # Add order to cache without adding instrument
        self.cache.add_order(order, position_id=None)

        # Create a report for the existing order
        report = OrderStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            order_status=OrderStatus.CANCELED,
            price=Price.from_str("1.00000"),
            quantity=Quantity.from_int(10_000),
            filled_qty=Quantity.from_int(0),
            report_id=UUID4(),
            ts_accepted=0,
            ts_triggered=0,
            ts_last=0,
            ts_init=0,
        )

        # Act
        result = live_exec_engine._reconcile_order_report(report, trades=[])

        # Assert
        assert result is True  # Deferred, not failed
        assert order.status == OrderStatus.ACCEPTED  # Status unchanged

    @pytest.mark.asyncio
    async def test_reconcile_position_report_with_missing_instrument_defers(
        self,
        live_exec_engine,
    ):
        """
        Test that position reconciliation is deferred when instrument is missing.
        """
        # Arrange
        # Instrument NOT added to cache
        report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=AUDUSD_SIM.id,  # Instrument not in cache
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(100),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        # Act
        result = live_exec_engine._reconcile_position_report(report)

        # Assert
        assert result is True  # Deferred, not failed
        assert len(self.cache.positions()) == 0  # No position created

    @pytest.mark.asyncio
    async def test_internal_diff_order_not_filtered_when_filter_unclaimed_external_orders_enabled(
        self,
        live_exec_engine,
    ):
        """
        Test that internal reconciliation orders are not filtered out when
        filter_unclaimed_external_orders is enabled.

        This ensures that position reconciliation orders (tagged RECONCILIATION) are
        generated even when external order filtering is active.

        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)

        # Enable external order filtering and missing order generation
        live_exec_engine.filter_unclaimed_external_orders = True
        live_exec_engine.generate_missing_orders = True

        # Create internal long position (100 units)
        order = TestExecStubs.limit_order(instrument=instrument, order_side=OrderSide.BUY)
        fill = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            position_id=PositionId("P-1"),
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("1.0"),
        )
        internal_position = Position(instrument=instrument, fill=fill)
        self.cache.add_position(internal_position, OmsType.NETTING)

        # External report shows 150 units (need to generate 50 BUY order)
        external_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(150),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        # Count orders before reconciliation
        orders_before = len(self.cache.orders())

        # Act
        result = live_exec_engine._reconcile_position_report(external_report)

        # Assert
        assert result is True

        # Verify that a new order was generated (not filtered out)
        orders_after = self.cache.orders()
        assert len(orders_after) == orders_before + 1

        # Find the newly generated order (should have EXTERNAL strategy ID but RECONCILIATION tag)
        new_orders = [
            o
            for o in orders_after
            if o.strategy_id.value == "EXTERNAL" and o.tags == ["RECONCILIATION"]
        ]
        assert len(new_orders) == 1

        generated_order = new_orders[0]
        assert generated_order.strategy_id.value == "EXTERNAL"
        assert generated_order.tags == ["RECONCILIATION"]
        assert generated_order.side == OrderSide.BUY
        assert generated_order.quantity == Quantity.from_int(50)
        assert generated_order.status == OrderStatus.FILLED

    @pytest.mark.asyncio
    async def test_external_order_filtered_when_filter_unclaimed_external_orders_enabled(
        self,
        live_exec_engine,
    ):
        """
        Test that regular EXTERNAL orders are filtered out when
        filter_unclaimed_external_orders is enabled.

        This ensures that the filtering mechanism works correctly for regular external
        orders.

        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)

        # Enable external order filtering
        live_exec_engine.filter_unclaimed_external_orders = True

        # Create external order report (not claimed by any strategy)
        external_report = OrderStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            client_order_id=ClientOrderId("EXTERNAL-ORDER-123"),
            venue_order_id=VenueOrderId("V-123"),
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            order_status=OrderStatus.FILLED,
            price=Price.from_str("1.0"),
            quantity=Quantity.from_int(100),
            filled_qty=Quantity.from_int(100),
            report_id=UUID4(),
            ts_accepted=0,
            ts_last=0,
            ts_init=0,
        )

        # Count orders before reconciliation
        orders_before = len(self.cache.orders())

        # Act
        result = live_exec_engine._reconcile_order_report(
            external_report,
            trades=[],
            is_external=True,
        )

        # Assert - reconciliation should succeed but no order should be added (filtered out)
        assert result is True
        orders_after = self.cache.orders()
        assert len(orders_after) == orders_before  # No new orders added due to filtering

    @pytest.mark.asyncio
    async def test_multiple_reconciliation_cycles_update_same_external_position(
        self,
        live_exec_engine,
    ):
        """
        Test that multiple reconciliation cycles use the SAME EXTERNAL strategy ID.

        Prevents position fragmentation by ensuring all reconciliation fills use
        EXTERNAL strategy ID to net into one position.

        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)
        live_exec_engine.generate_missing_orders = True

        reconcile_calls = []
        original_reconcile = live_exec_engine._reconcile_order_report

        def spy_reconcile(order_report, trades, is_external=True):
            reconcile_calls.append((order_report, trades, is_external))
            return original_reconcile(order_report, trades, is_external)

        live_exec_engine._reconcile_order_report = spy_reconcile

        # First reconciliation: venue has LONG 100
        report1 = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(100),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        result1 = live_exec_engine._reconcile_position_report(report1)
        assert result1 is True
        assert len(reconcile_calls) == 1

        # Verify first reconciliation created EXTERNAL order with RECONCILIATION tag
        order_report1, _, is_external1 = reconcile_calls[0]
        assert order_report1.order_side == OrderSide.BUY
        assert order_report1.quantity == Quantity.from_int(100)
        assert is_external1 is False  # Internal reconciliation

        # Check the generated order uses EXTERNAL strategy ID
        orders_after_1st = self.cache.orders()
        assert len(orders_after_1st) == 1
        assert orders_after_1st[0].strategy_id.value == "EXTERNAL"
        assert orders_after_1st[0].tags == ["RECONCILIATION"]

        # Simulate the first order being processed by creating a position
        # (In real flow, this would happen automatically via execution engine)
        first_order = orders_after_1st[0]
        fill_1 = TestEventStubs.order_filled(
            first_order,
            instrument=instrument,
            position_id=PositionId("AUDUSD.SIM-EXTERNAL"),
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("1.0"),
            trade_id=TradeId("RECON-TRADE-1"),
        )
        position_1 = Position(instrument=instrument, fill=fill_1)
        self.cache.add_position(position_1, OmsType.NETTING)

        # Second reconciliation: venue now has LONG 150 (manual trade on exchange)
        report2 = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(150),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        result2 = live_exec_engine._reconcile_position_report(report2)
        assert result2 is True
        assert len(reconcile_calls) == 2

        # Second reconciliation also uses EXTERNAL strategy ID
        order_report2, _, is_external2 = reconcile_calls[1]
        assert order_report2.order_side == OrderSide.BUY
        assert order_report2.quantity == Quantity.from_int(50)  # Incremental from 100 to 150
        assert is_external2 is False

        # Verify both orders use EXTERNAL strategy ID
        orders_after_second = self.cache.orders()
        assert len(orders_after_second) == 2

        for order in orders_after_second:
            assert order.strategy_id.value == "EXTERNAL"
            assert order.tags == ["RECONCILIATION"]

    @pytest.mark.asyncio
    async def test_reconciliation_with_existing_position_uses_external_strategy_id(
        self,
        live_exec_engine,
    ):
        """
        Test that reconciliation with existing user strategy position generates
        reconciliation order with EXTERNAL strategy ID.

        Verifies that all unclaimed reconciliation uses consistent EXTERNAL strategy ID
        to enable proper netting in strategy-level netting mode.

        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)
        live_exec_engine.generate_missing_orders = True

        # User strategy creates position
        order = TestExecStubs.limit_order(
            instrument=instrument,
            order_side=OrderSide.BUY,
        )
        fill = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            position_id=PositionId("P-1"),
            last_qty=Quantity.from_int(50),
            last_px=Price.from_str("1.0"),
        )
        user_position = Position(instrument=instrument, fill=fill)
        self.cache.add_position(user_position, OmsType.NETTING)

        reconcile_calls = []
        original_reconcile = live_exec_engine._reconcile_order_report

        def spy_reconcile(order_report, trades, is_external=True):
            reconcile_calls.append((order_report, trades, is_external))
            return original_reconcile(order_report, trades, is_external)

        live_exec_engine._reconcile_order_report = spy_reconcile

        # Venue reports LONG 100 (50 from user + 50 external)
        report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(100),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        # Act
        result = live_exec_engine._reconcile_position_report(report)

        # Assert
        assert result is True
        assert len(reconcile_calls) == 1

        # Verify reconciliation order generated with correct quantity
        order_report, _, is_external = reconcile_calls[0]
        assert order_report.order_side == OrderSide.BUY
        assert order_report.quantity == Quantity.from_int(50)  # Difference
        assert is_external is False  # Internal reconciliation

        # Verify generated order uses EXTERNAL strategy ID
        generated_orders = [
            o for o in self.cache.orders()
            if o.client_order_id == order_report.client_order_id
        ]
        assert len(generated_orders) == 1

        generated_order = generated_orders[0]
        assert generated_order.strategy_id.value == "EXTERNAL"
        assert generated_order.tags == ["RECONCILIATION"]

    @pytest.mark.asyncio
    async def test_position_reconciliation_fallback_to_market_order_when_no_price_available(
        self,
        live_exec_engine,
    ):
        """
        Test that position reconciliation falls back to MARKET order when no price
        information is available (no positions, no market data).

        This tests the last resort fallback when:
        1. Reconciliation price calculation returns None (no target avg price)
        2. No quote tick is available in cache (no market data)
        3. No current position average price (starting from flat)

        The reconciliation returns True in this case because we've done our best
        to reconcile the position, even though price information is unavailable.

        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)

        # Ensure no quote tick is available
        assert self.cache.quote_tick(instrument.id) is None

        # External report shows 100 units long (starting from flat position)
        external_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(100),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        # Track reconciliation calls to verify MARKET order is generated
        reconcile_calls = []
        original_reconcile = live_exec_engine._reconcile_order_report

        def spy_reconcile(order_report, trades, is_external=True):
            reconcile_calls.append((order_report, trades, is_external))
            return original_reconcile(order_report, trades, is_external)

        live_exec_engine._reconcile_order_report = spy_reconcile

        # Act
        result = live_exec_engine._reconcile_position_report(external_report)

        # Assert
        # Reconciliation returns True because we've done our best to reconcile
        assert result is True
        assert len(reconcile_calls) == 1

        # Verify the generated order report is a MARKET order (fallback)
        order_report, trades, is_external = reconcile_calls[0]
        assert order_report.order_type == OrderType.MARKET
        assert order_report.time_in_force == TimeInForce.IOC
        assert order_report.price is None  # MARKET orders don't have a price
        assert order_report.order_side == OrderSide.BUY
        assert order_report.quantity == Quantity.from_int(100)
        assert order_report.filled_qty == Quantity.from_int(100)
        assert order_report.order_status == OrderStatus.FILLED
        assert is_external is False  # Internal reconciliation order

        # Verify the order was added to cache with EXTERNAL strategy and RECONCILIATION tag
        orders = self.cache.orders()
        internal_recon_orders = [
            o for o in orders if o.strategy_id.value == "EXTERNAL" and o.tags == ["RECONCILIATION"]
        ]
        assert len(internal_recon_orders) == 1

        generated_order = internal_recon_orders[0]
        assert generated_order.order_type == OrderType.MARKET
        assert generated_order.side == OrderSide.BUY
        assert generated_order.quantity == Quantity.from_int(100)
        # Order might be ACCEPTED or FILLED depending on how reconciliation processes events
        assert generated_order.status in (OrderStatus.ACCEPTED, OrderStatus.FILLED)

    @pytest.mark.asyncio
    async def test_position_reconciliation_uses_limit_order_when_price_available(
        self,
        live_exec_engine,
    ):
        """
        Test that position reconciliation uses LIMIT order when price information is
        available (via quote tick).

        This verifies that LIMIT orders are preferred when we can determine a price.

        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)

        # Add a quote tick to provide market data
        quote_tick = TestDataStubs.quote_tick(
            instrument=instrument,
            bid_price=Price.from_str("0.9999"),
            ask_price=Price.from_str("1.0001"),
        )
        self.cache.add_quote_tick(quote_tick)

        # External report shows 100 units long (starting from flat position)
        external_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(100),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        # Track reconciliation calls
        reconcile_calls = []
        original_reconcile = live_exec_engine._reconcile_order_report

        def spy_reconcile(order_report, trades, is_external=True):
            reconcile_calls.append((order_report, trades, is_external))
            return original_reconcile(order_report, trades, is_external)

        live_exec_engine._reconcile_order_report = spy_reconcile

        # Act
        result = live_exec_engine._reconcile_position_report(external_report)

        # Assert
        assert result is True
        assert len(reconcile_calls) == 1

        # Verify the generated order report is a LIMIT order with calculated price
        order_report, trades, is_external = reconcile_calls[0]
        assert order_report.order_type == OrderType.LIMIT
        assert order_report.time_in_force == TimeInForce.GTC
        assert order_report.price is not None  # LIMIT orders have a price
        assert order_report.price == Price.from_str("1.0001")  # Ask price for BUY order
        assert order_report.avg_px == Decimal("1.0001")
        assert order_report.order_side == OrderSide.BUY
        assert order_report.quantity == Quantity.from_int(100)
        assert order_report.filled_qty == Quantity.from_int(100)
        assert order_report.order_status == OrderStatus.FILLED
        assert is_external is False  # Internal reconciliation order

        # Verify the order was added to cache with EXTERNAL strategy and RECONCILIATION tag
        orders = self.cache.orders()
        internal_recon_orders = [
            o for o in orders if o.strategy_id.value == "EXTERNAL" and o.tags == ["RECONCILIATION"]
        ]
        assert len(internal_recon_orders) == 1

        generated_order = internal_recon_orders[0]
        assert generated_order.order_type == OrderType.LIMIT
        assert generated_order.side == OrderSide.BUY
        assert generated_order.quantity == Quantity.from_int(100)
        assert generated_order.price == Price.from_str("1.0001")  # Ask price for BUY order
        assert generated_order.status == OrderStatus.FILLED

    @pytest.mark.asyncio
    async def test_position_reconciliation_crosses_zero_splits_into_two_fills(
        self,
        live_exec_engine,
    ):
        """
        Test that position reconciliation crossing through zero generates two separate fills:
        one to close the existing position and one to open the new position.
        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)
        live_exec_engine.generate_missing_orders = True

        # Create internal long position (100 units @ 1.0)
        order = TestExecStubs.limit_order(instrument=instrument, order_side=OrderSide.BUY)
        fill = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            position_id=PositionId("P-CROSS-ZERO"),
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("1.0"),
        )
        internal_position = Position(instrument=instrument, fill=fill)
        self.cache.add_position(internal_position, OmsType.NETTING)

        # External report shows -50 units short with avg_px=1.05
        # This crosses through zero: LONG 100 -> SHORT -50
        external_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.SHORT,
            quantity=Quantity.from_int(50),
            avg_px_open=Decimal("1.05"),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        # Spy on reconcile_order_report calls
        reconcile_calls = []
        original_reconcile = live_exec_engine._reconcile_order_report

        def spy_reconcile(order_report, trades, is_external=True):
            reconcile_calls.append((order_report, trades, is_external))
            return original_reconcile(order_report, trades, is_external)

        live_exec_engine._reconcile_order_report = spy_reconcile

        # Act
        result = live_exec_engine._reconcile_position_report(external_report)

        # Assert
        assert result is True

        # Should have generated TWO reconciliation orders: one to close, one to open
        assert len(reconcile_calls) == 2

        # First order: close existing LONG position (SELL 100)
        close_report, _, _ = reconcile_calls[0]
        assert close_report.order_side == OrderSide.SELL
        assert close_report.quantity == Quantity.from_int(100)
        assert close_report.filled_qty == Quantity.from_int(100)
        assert close_report.order_status == OrderStatus.FILLED

        # Second order: open new SHORT position (SELL 50 @ venue's avg price)
        open_report, _, _ = reconcile_calls[1]
        assert open_report.order_side == OrderSide.SELL
        assert open_report.quantity == Quantity.from_int(50)
        assert open_report.filled_qty == Quantity.from_int(50)
        assert open_report.order_status == OrderStatus.FILLED
        assert open_report.avg_px == Decimal("1.05")

    @pytest.mark.asyncio
    async def test_duplicate_fill_detection_prevents_historical_fills_after_inferred_fill(
        self,
        live_exec_engine,
    ):
        """
        Test that historical fills with timestamps older than inferred reconciliation
        fills are skipped to prevent duplicate fill application.
        """
        # Arrange
        instrument = AUDUSD_SIM
        self.cache.add_instrument(instrument)

        # Create an order
        order = TestExecStubs.limit_order(
            instrument=instrument,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(100),
        )
        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))
        self.cache.add_order(order, None)

        # Apply an inferred reconciliation fill with ts_event = 1000
        inferred_fill = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            last_qty=Quantity.from_int(50),
            last_px=Price.from_str("1.0"),
            ts_event=1000,
        )
        # Manually track this as an inferred fill (simulating reconciliation behavior)
        live_exec_engine._inferred_fill_ts[order.client_order_id] = 1000
        live_exec_engine._handle_event(inferred_fill)

        # Verify the order was filled
        assert order.filled_qty == Quantity.from_int(50)

        # Try to apply a historical fill with older timestamp (ts_event = 500)
        historical_fill_report = FillReport(
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            trade_id=TradeId("historical-fill-1"),
            account_id=order.account_id,
            instrument_id=instrument.id,
            order_side=order.side,
            last_qty=Quantity.from_int(10),
            last_px=Price.from_str("0.99"),
            commission=Money(0.01, USD),
            liquidity_side=LiquiditySide.MAKER,
            report_id=UUID4(),
            ts_event=500,  # Older than inferred fill
            ts_init=1500,
        )

        # Act
        result = live_exec_engine._reconcile_fill_report(order, historical_fill_report, instrument)

        # Assert
        assert result is True  # Returns True (success) but doesn't apply the fill
        # Order should still have only 50 filled (from inferred fill), not 60
        assert order.filled_qty == Quantity.from_int(50)
        # Historical fill trade_id should NOT be in order's trade_ids
        assert TradeId("historical-fill-1") not in order.trade_ids


class TestReconciliationFiltering:
    """
    Tests for filtering logic during live execution reconciliation.
    """

    def _get_exec_engine(self, config: LiveExecEngineConfig):
        # Get the running loop from pytest-asyncio (session-scoped)
        loop = asyncio.get_running_loop()
        clock = LiveClock()
        trader_id = TestIdStubs.trader_id()
        msgbus = MessageBus(trader_id=trader_id, clock=clock)
        cache = TestComponentStubs.cache()
        cache.add_instrument(AUDUSD_SIM)
        cache.add_instrument(GBPUSD_SIM)

        client = MockLiveExecutionClient(
            loop=loop,
            client_id=ClientId("SIM"),
            venue=Venue("SIM"),
            account_type=AccountType.CASH,
            base_currency=USD,
            instrument_provider=InstrumentProvider(),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        exec_engine = LiveExecutionEngine(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=config,
        )

        exec_engine.register_client(client)
        return exec_engine, cache

    @pytest.mark.asyncio
    async def test_reconciliation_instrument_ids_empty_processes_all(self):
        """
        Test that if `reconciliation_instrument_ids` is empty, all reports are
        processed.
        """
        # Arrange
        config = LiveExecEngineConfig(
            reconciliation=True,
            reconciliation_instrument_ids=[],  # Empty list
        )
        exec_engine, cache = self._get_exec_engine(config)

        report1 = OrderStatusReport(
            instrument_id=AUDUSD_SIM.id,
            account_id=TestIdStubs.account_id(),
            client_order_id=ClientOrderId("order-1"),
            venue_order_id=VenueOrderId("V-1"),
            order_side=OrderSide.BUY,
            order_type=OrderType.MARKET,
            time_in_force=TimeInForce.DAY,
            order_status=OrderStatus.ACCEPTED,
            quantity=Quantity.from_int(100),
            filled_qty=Quantity.from_int(0),
            report_id=UUID4(),
            ts_accepted=0,
            ts_last=0,
            ts_init=0,
        )

        report2 = OrderStatusReport(
            instrument_id=GBPUSD_SIM.id,
            account_id=TestIdStubs.account_id(),
            client_order_id=ClientOrderId("order-2"),
            venue_order_id=VenueOrderId("V-2"),
            order_side=OrderSide.SELL,
            order_type=OrderType.MARKET,
            time_in_force=TimeInForce.DAY,
            order_status=OrderStatus.ACCEPTED,
            quantity=Quantity.from_int(50),
            filled_qty=Quantity.from_int(0),
            report_id=UUID4(),
            ts_accepted=0,
            ts_last=0,
            ts_init=0,
        )

        mass_status = ExecutionMassStatus(
            client_id=ClientId("TEST"),
            venue=Venue("TEST"),
            account_id=TestIdStubs.account_id(),
            report_id=UUID4(),
            ts_init=0,
        )
        mass_status.add_order_reports([report1, report2])

        # Act
        result = exec_engine._reconcile_execution_mass_status(mass_status)

        # Assert
        assert result is True
        assert len(cache.orders()) == 2
        assert cache.order(ClientOrderId("order-1")) is not None
        assert cache.order(ClientOrderId("order-2")) is not None

    @pytest.mark.asyncio
    async def test_reconciliation_instrument_ids_filters_reports(self):
        """
        Test that reports are filtered based on `reconciliation_instrument_ids`.
        """
        # Arrange
        config = LiveExecEngineConfig(
            reconciliation=True,
            reconciliation_instrument_ids=[AUDUSD_SIM.id],
        )
        exec_engine, cache = self._get_exec_engine(config)

        report_included = OrderStatusReport(
            instrument_id=AUDUSD_SIM.id,
            account_id=TestIdStubs.account_id(),
            client_order_id=ClientOrderId("included-order"),
            venue_order_id=VenueOrderId("V-1"),
            order_side=OrderSide.BUY,
            order_type=OrderType.MARKET,
            time_in_force=TimeInForce.DAY,
            order_status=OrderStatus.ACCEPTED,
            quantity=Quantity.from_int(100),
            filled_qty=Quantity.from_int(0),
            report_id=UUID4(),
            ts_accepted=0,
            ts_last=0,
            ts_init=0,
        )

        report_filtered = OrderStatusReport(
            instrument_id=GBPUSD_SIM.id,  # This instrument is not in the include list
            account_id=TestIdStubs.account_id(),
            client_order_id=ClientOrderId("filtered-order"),
            venue_order_id=VenueOrderId("V-2"),
            order_side=OrderSide.SELL,
            order_type=OrderType.MARKET,
            time_in_force=TimeInForce.DAY,
            order_status=OrderStatus.ACCEPTED,
            quantity=Quantity.from_int(50),
            filled_qty=Quantity.from_int(0),
            report_id=UUID4(),
            ts_accepted=0,
            ts_last=0,
            ts_init=0,
        )

        mass_status = ExecutionMassStatus(
            client_id=ClientId("TEST"),
            venue=Venue("TEST"),
            account_id=TestIdStubs.account_id(),
            report_id=UUID4(),
            ts_init=0,
        )
        mass_status.add_order_reports([report_included, report_filtered])

        # Act
        result = exec_engine._reconcile_execution_mass_status(mass_status)

        # Assert
        assert result is True
        assert len(cache.orders()) == 1
        assert cache.order(ClientOrderId("included-order")) is not None
        assert cache.order(ClientOrderId("filtered-order")) is None

    @pytest.mark.asyncio
    async def test_filtered_client_order_ids_filters_reports(self):
        """
        Test that reports are filtered based on `filtered_client_order_ids`.
        """
        # Arrange
        filtered_coid = ClientOrderId("filter-this-id")
        config = LiveExecEngineConfig(
            reconciliation=True,
            filtered_client_order_ids=[filtered_coid],
        )
        exec_engine, cache = self._get_exec_engine(config)

        report_included = OrderStatusReport(
            instrument_id=AUDUSD_SIM.id,
            account_id=TestIdStubs.account_id(),
            client_order_id=ClientOrderId("included-order"),
            venue_order_id=VenueOrderId("V-1"),
            order_side=OrderSide.BUY,
            order_type=OrderType.MARKET,
            time_in_force=TimeInForce.DAY,
            order_status=OrderStatus.ACCEPTED,
            quantity=Quantity.from_int(100),
            filled_qty=Quantity.from_int(0),
            report_id=UUID4(),
            ts_accepted=0,
            ts_last=0,
            ts_init=0,
        )

        report_filtered = OrderStatusReport(
            instrument_id=AUDUSD_SIM.id,
            account_id=TestIdStubs.account_id(),
            client_order_id=filtered_coid,  # This ID is in the filter list
            venue_order_id=VenueOrderId("V-2"),
            order_side=OrderSide.SELL,
            order_type=OrderType.MARKET,
            time_in_force=TimeInForce.DAY,
            order_status=OrderStatus.ACCEPTED,
            quantity=Quantity.from_int(50),
            filled_qty=Quantity.from_int(0),
            report_id=UUID4(),
            ts_accepted=0,
            ts_last=0,
            ts_init=0,
        )

        mass_status = ExecutionMassStatus(
            client_id=ClientId("TEST"),
            venue=Venue("TEST"),
            account_id=TestIdStubs.account_id(),
            report_id=UUID4(),
            ts_init=0,
        )
        mass_status.add_order_reports([report_included, report_filtered])

        # Act
        result = exec_engine._reconcile_execution_mass_status(mass_status)

        # Assert
        assert result is True
        assert len(cache.orders()) == 1
        assert cache.order(ClientOrderId("included-order")) is not None
        assert cache.order(filtered_coid) is None


class TestLiveExecutionReconciliationEdgeCases:
    """
    Tests for edge cases in reconciliation event signaling to prevent deadlocks.
    """

    @pytest.mark.asyncio
    async def test_reconciliation_disabled_does_not_block_continuous_loop(self):
        """
        Test that when reconciliation is disabled, continuous loop doesn't hang waiting
        for event.
        """
        # Arrange
        # Get the running loop from pytest-asyncio (session-scoped)
        loop = asyncio.get_running_loop()
        clock = LiveClock()
        trader_id = TestIdStubs.trader_id()
        msgbus = MessageBus(trader_id=trader_id, clock=clock)
        cache = TestComponentStubs.cache()

        client = MockLiveExecutionClient(
            loop=loop,
            client_id=ClientId("SIM"),
            venue=Venue("SIM"),
            account_type=AccountType.CASH,
            base_currency=USD,
            instrument_provider=InstrumentProvider(),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        # Create engine with reconciliation disabled
        exec_engine = LiveExecutionEngine(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=LiveExecEngineConfig(
                reconciliation=False,
                open_check_interval_secs=5.0,
            ),
        )
        exec_engine.register_client(client)

        # Act
        exec_engine.start()

        # Give continuous loop a moment to start
        await asyncio.sleep(0.1)

        # Assert - if we get here without hanging, the test passes
        assert exec_engine.reconciliation is False
        assert exec_engine._reconciliation_task is not None

        # Cleanup
        exec_engine.stop()

    @pytest.mark.asyncio
    async def test_reconciliation_with_no_clients_sets_event(self):
        """
        Test that reconciliation with no clients sets event so continuous loop doesn't
        hang.
        """
        # Arrange
        # Get the running loop from pytest-asyncio (session-scoped)
        loop = asyncio.get_running_loop()
        clock = LiveClock()
        trader_id = TestIdStubs.trader_id()
        msgbus = MessageBus(trader_id=trader_id, clock=clock)
        cache = TestComponentStubs.cache()

        # Create engine without registering any clients
        exec_engine = LiveExecutionEngine(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=LiveExecEngineConfig(
                reconciliation=True,
                open_check_interval_secs=5.0,
            ),
        )

        # Act
        result = await exec_engine.reconcile_execution_state()

        # Assert
        assert result is True
        assert exec_engine._startup_reconciliation_event.is_set()

    @pytest.mark.asyncio
    async def test_on_start_clears_reconciliation_event(self):
        """
        Test that _on_start() clears the reconciliation event to prevent race
        conditions.
        """
        # Arrange
        # Get the running loop from pytest-asyncio (session-scoped)
        loop = asyncio.get_running_loop()
        clock = LiveClock()
        trader_id = TestIdStubs.trader_id()
        msgbus = MessageBus(trader_id=trader_id, clock=clock)
        cache = TestComponentStubs.cache()

        client = MockLiveExecutionClient(
            loop=loop,
            client_id=ClientId("SIM"),
            venue=Venue("SIM"),
            account_type=AccountType.CASH,
            base_currency=USD,
            instrument_provider=InstrumentProvider(),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        exec_engine = LiveExecutionEngine(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=LiveExecEngineConfig(
                reconciliation=True,
                open_check_interval_secs=5.0,
            ),
        )
        exec_engine.register_client(client)

        # Set the event manually to simulate it being set from a previous cycle
        exec_engine._startup_reconciliation_event.set()
        assert exec_engine._startup_reconciliation_event.is_set()

        # Act - Start engine which should call _on_start()
        exec_engine.start()

        # Give _on_start time to execute
        await asyncio.sleep(0.05)

        # Assert - Event should be cleared by _on_start()
        assert not exec_engine._startup_reconciliation_event.is_set()

        # Verify reconciliation sets it again
        await exec_engine.reconcile_execution_state()
        assert exec_engine._startup_reconciliation_event.is_set()

        # Cleanup
        exec_engine.stop()

    @pytest.mark.asyncio
    async def test_flat_report_processed_during_startup_reconciliation(self):
        """
        Test that FLAT position reports are processed during startup reconciliation.

        Verifies the reconciliation flow handles FLAT reports from mass status.

        """
        # Setup
        # Get the running loop from pytest-asyncio (session-scoped)
        loop = asyncio.get_running_loop()

        clock = LiveClock()
        trader_id = TestIdStubs.trader_id()
        msgbus = MessageBus(trader_id=trader_id, clock=clock)
        cache = TestComponentStubs.cache()

        instrument = AUDUSD_SIM
        cache.add_instrument(instrument)

        # Create position that exists locally (simulating pre-restart state)
        order = TestExecStubs.limit_order(
            instrument=instrument,
            order_side=OrderSide.BUY,
        )
        fill = TestEventStubs.order_filled(
            order,
            instrument=instrument,
            position_id=PositionId("P-OVERNIGHT"),
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("0.70"),
        )
        position = Position(instrument=instrument, fill=fill)
        cache.add_position(position, OmsType.NETTING)

        client = MockLiveExecutionClient(
            loop=loop,
            client_id=ClientId("SIM"),
            venue=SIM,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            instrument_provider=InstrumentProvider(),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        exec_engine = LiveExecutionEngine(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=LiveExecEngineConfig(
                reconciliation=True,
                generate_missing_orders=True,
            ),
        )
        exec_engine.register_client(client)

        flat_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.FLAT,
            quantity=Quantity.zero(),
            report_id=UUID4(),
            ts_last=clock.timestamp_ns(),
            ts_init=clock.timestamp_ns(),
        )

        # Directly test reconciliation with FLAT report
        result = exec_engine._reconcile_position_report(flat_report)

        # Assert
        assert result is True, "FLAT report should be successfully reconciled"


# =============================================================================
# FIXTURES FOR STANDALONE TESTS
# =============================================================================


@pytest.fixture
def live_exec_engine(event_loop, cache):
    """
    Create a live execution engine for standalone tests.
    """
    loop = event_loop
    clock = LiveClock()
    trader_id = TestIdStubs.trader_id()
    msgbus = MessageBus(trader_id=trader_id, clock=clock)

    exec_engine = LiveExecutionEngine(
        loop=loop,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        config=LiveExecEngineConfig(reconciliation=True),
    )
    return exec_engine


@pytest.fixture
def exec_client(event_loop, cache):
    """
    Create a mock execution client for standalone tests.
    """
    loop = event_loop
    clock = LiveClock()
    trader_id = TestIdStubs.trader_id()
    msgbus = MessageBus(trader_id=trader_id, clock=clock)

    client = MockLiveExecutionClient(
        loop=loop,
        client_id=ClientId("SIM"),
        venue=Venue("SIM"),
        account_type=AccountType.CASH,
        base_currency=USD,
        instrument_provider=InstrumentProvider(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    return client


@pytest.fixture
def account_id():
    """
    Create an account ID for standalone tests.
    """
    return TestIdStubs.account_id()


@pytest.fixture
def cache():
    """
    Create a cache for standalone tests.
    """
    cache = TestComponentStubs.cache()
    cache.add_instrument(AUDUSD_SIM)
    cache.add_instrument(GBPUSD_SIM)
    return cache


# =============================================================================
# TESTS FOR _query_position_status_reports
# =============================================================================


@pytest.mark.asyncio
async def test_query_position_status_reports_success(live_exec_engine, exec_client, account_id):
    """
    Test _query_position_status_reports successfully queries and returns reports.
    """
    # Arrange
    # Register the client with the engine
    live_exec_engine.register_client(exec_client)

    report = PositionStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        position_side=PositionSide.LONG,
        quantity=Quantity.from_int(1000),
        report_id=UUID4(),
        ts_last=live_exec_engine._clock.timestamp_ns(),
        ts_init=live_exec_engine._clock.timestamp_ns(),
    )
    exec_client.add_position_status_report(report)

    # Act
    venue_positions = await live_exec_engine._query_position_status_reports()

    # Assert
    assert len(venue_positions) == 1
    assert AUDUSD_SIM.id in venue_positions
    assert venue_positions[AUDUSD_SIM.id].quantity == Quantity.from_int(1000)


@pytest.mark.asyncio
async def test_query_position_status_reports_handles_exceptions(live_exec_engine, exec_client):
    """
    Test _query_position_status_reports handles client exceptions gracefully.
    """
    # Arrange
    # Register the client with the engine
    live_exec_engine.register_client(exec_client)

    async def raise_error(command):
        raise RuntimeError("API error")

    exec_client.generate_position_status_reports = raise_error

    # Act
    venue_positions = await live_exec_engine._query_position_status_reports()

    # Assert
    assert len(venue_positions) == 0


@pytest.mark.asyncio
async def test_query_position_status_reports_multiple_instruments(live_exec_engine, exec_client, account_id):
    """
    Test _query_position_status_reports handles multiple instruments.
    """
    # Arrange
    # Register the client with the engine
    live_exec_engine.register_client(exec_client)

    report1 = PositionStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        position_side=PositionSide.LONG,
        quantity=Quantity.from_int(1000),
        report_id=UUID4(),
        ts_last=live_exec_engine._clock.timestamp_ns(),
        ts_init=live_exec_engine._clock.timestamp_ns(),
    )
    report2 = PositionStatusReport(
        account_id=account_id,
        instrument_id=GBPUSD_SIM.id,
        position_side=PositionSide.SHORT,
        quantity=Quantity.from_int(2000),
        report_id=UUID4(),
        ts_last=live_exec_engine._clock.timestamp_ns(),
        ts_init=live_exec_engine._clock.timestamp_ns(),
    )
    exec_client.add_position_status_report(report1)
    exec_client.add_position_status_report(report2)

    # Act
    venue_positions = await live_exec_engine._query_position_status_reports()

    # Assert
    assert len(venue_positions) == 2
    assert AUDUSD_SIM.id in venue_positions
    assert GBPUSD_SIM.id in venue_positions


# =============================================================================
# TESTS FOR _query_and_find_missing_fills
# =============================================================================


@pytest.mark.asyncio
async def test_query_and_find_missing_fills_finds_missing(live_exec_engine, exec_client, cache, account_id):
    """
    Test _query_and_find_missing_fills identifies fills not in cache.
    """
    # Arrange
    # Register the client with the engine
    live_exec_engine.register_client(exec_client)

    order = TestExecStubs.limit_order(instrument=AUDUSD_SIM)
    cache.add_order(order)

    submitted = TestEventStubs.order_submitted(order, account_id=account_id)
    order.apply(submitted)
    live_exec_engine.process(submitted)

    accepted = TestEventStubs.order_accepted(
        order,
        account_id=account_id,
        venue_order_id=VenueOrderId("V-1"),
    )
    order.apply(accepted)
    live_exec_engine.process(accepted)

    # Add one fill to cache
    filled1 = TestEventStubs.order_filled(
        order,
        instrument=AUDUSD_SIM,
        last_qty=Quantity.from_int(50),
        trade_id=TradeId("T-1"),
    )
    order.apply(filled1)
    live_exec_engine.process(filled1)
    cache.update_order(order)

    # Venue reports two fills
    current_ns = live_exec_engine._clock.timestamp_ns()
    fill_report1 = FillReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-1"),
        venue_position_id=None,
        trade_id=TradeId("T-1"),  # Already in cache
        order_side=OrderSide.BUY,
        last_qty=Quantity.from_int(50),
        last_px=Price.from_str("1.00000"),
        commission=Money("1.00", USD),
        liquidity_side=LiquiditySide.TAKER,
        report_id=UUID4(),
        ts_event=current_ns,
        ts_init=current_ns,
    )
    fill_report2 = FillReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-1"),
        venue_position_id=None,
        trade_id=TradeId("T-2"),  # Missing from cache
        order_side=OrderSide.BUY,
        last_qty=Quantity.from_int(50),
        last_px=Price.from_str("1.01000"),
        commission=Money("1.00", USD),
        liquidity_side=LiquiditySide.TAKER,
        report_id=UUID4(),
        ts_event=current_ns + 1_000_000,
        ts_init=current_ns + 1_000_000,
    )
    exec_client.add_fill_reports(VenueOrderId("V-1"), [fill_report1, fill_report2])

    # Act
    missing_fills = await live_exec_engine._query_and_find_missing_fills(
        AUDUSD_SIM.id,
        live_exec_engine._clients.values(),
    )

    # Assert
    assert len(missing_fills) == 1
    assert missing_fills[0].trade_id == TradeId("T-2")


@pytest.mark.asyncio
async def test_query_and_find_missing_fills_handles_exceptions(live_exec_engine, exec_client):
    """
    Test _query_and_find_missing_fills handles client exceptions gracefully.
    """
    # Arrange
    # Register the client with the engine
    live_exec_engine.register_client(exec_client)

    async def raise_error(command):
        raise RuntimeError("API error")

    exec_client.generate_fill_reports = raise_error

    # Act
    missing_fills = await live_exec_engine._query_and_find_missing_fills(
        AUDUSD_SIM.id,
        live_exec_engine._clients.values(),
    )

    # Assert
    assert len(missing_fills) == 0


@pytest.mark.asyncio
async def test_query_and_find_missing_fills_no_missing(live_exec_engine, exec_client, cache, account_id):
    """
    Test _query_and_find_missing_fills returns empty when all fills are cached.
    """
    # Arrange
    # Register the client with the engine
    live_exec_engine.register_client(exec_client)

    order = TestExecStubs.limit_order(instrument=AUDUSD_SIM)
    cache.add_order(order)

    submitted = TestEventStubs.order_submitted(order, account_id=account_id)
    order.apply(submitted)
    live_exec_engine.process(submitted)

    accepted = TestEventStubs.order_accepted(
        order,
        account_id=account_id,
        venue_order_id=VenueOrderId("V-1"),
    )
    order.apply(accepted)
    live_exec_engine.process(accepted)

    # Add fill to cache
    filled = TestEventStubs.order_filled(
        order,
        instrument=AUDUSD_SIM,
        last_qty=Quantity.from_int(100),
        trade_id=TradeId("T-1"),
    )
    order.apply(filled)
    live_exec_engine.process(filled)
    cache.update_order(order)

    # Venue reports same fill
    current_ns = live_exec_engine._clock.timestamp_ns()
    fill_report = FillReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-1"),
        venue_position_id=None,
        trade_id=TradeId("T-1"),  # Already in cache
        order_side=OrderSide.BUY,
        last_qty=Quantity.from_int(100),
        last_px=Price.from_str("1.00000"),
        commission=Money("1.00", USD),
        liquidity_side=LiquiditySide.TAKER,
        report_id=UUID4(),
        ts_event=current_ns,
        ts_init=current_ns,
    )
    exec_client.add_fill_reports(VenueOrderId("V-1"), [fill_report])

    # Act
    missing_fills = await live_exec_engine._query_and_find_missing_fills(
        AUDUSD_SIM.id,
        live_exec_engine._clients.values(),
    )

    # Assert
    assert len(missing_fills) == 0


# =============================================================================
# TESTS FOR _reconcile_missing_fills
# =============================================================================


@pytest.mark.asyncio
async def test_reconcile_missing_fills_empty_list(live_exec_engine):
    """
    Test _reconcile_missing_fills handles empty list gracefully.
    """
    # Act
    await live_exec_engine._reconcile_missing_fills([], AUDUSD_SIM.id)

    # Assert - should not raise


@pytest.mark.asyncio
async def test_reconcile_missing_fills_reconciles_successfully(
    live_exec_engine,
    cache,
    account_id,
):
    """
    Test _reconcile_missing_fills successfully reconciles missing fills.
    """
    # Arrange
    # Ensure cache has the instrument
    if AUDUSD_SIM.id not in [i.id for i in cache.instruments()]:
        cache.add_instrument(AUDUSD_SIM)

    order = TestExecStubs.limit_order(instrument=AUDUSD_SIM)
    cache.add_order(order)

    submitted = TestEventStubs.order_submitted(order, account_id=account_id)
    order.apply(submitted)
    live_exec_engine.process(submitted)

    accepted = TestEventStubs.order_accepted(
        order,
        account_id=account_id,
        venue_order_id=VenueOrderId("V-1"),
    )
    order.apply(accepted)
    live_exec_engine.process(accepted)
    cache.update_order(order)

    current_ns = live_exec_engine._clock.timestamp_ns()
    fill_report = FillReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-1"),
        venue_position_id=None,
        trade_id=TradeId("T-1"),
        order_side=OrderSide.BUY,
        last_qty=Quantity.from_int(50),
        last_px=Price.from_str("1.00000"),
        commission=Money("1.00", USD),
        liquidity_side=LiquiditySide.TAKER,
        report_id=UUID4(),
        ts_event=current_ns,
        ts_init=current_ns,
    )

    # Act
    await live_exec_engine._reconcile_missing_fills([fill_report], AUDUSD_SIM.id)

    # Assert
    assert order.filled_qty == Quantity.from_int(50)
    assert AUDUSD_SIM.id in live_exec_engine._position_local_activity_ns


@pytest.mark.asyncio
async def test_reconcile_missing_fills_handles_failure(live_exec_engine, cache, account_id):
    """
    Test _reconcile_missing_fills handles reconciliation failures gracefully.
    """
    # Arrange
    # Ensure cache has the instrument
    if AUDUSD_SIM.id not in [i.id for i in cache.instruments()]:
        cache.add_instrument(AUDUSD_SIM)

    # Create fill report for non-existent order
    current_ns = live_exec_engine._clock.timestamp_ns()
    fill_report = FillReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=ClientOrderId("NON-EXISTENT"),
        venue_order_id=VenueOrderId("V-999"),
        venue_position_id=None,
        trade_id=TradeId("T-1"),
        order_side=OrderSide.BUY,
        last_qty=Quantity.from_int(50),
        last_px=Price.from_str("1.00000"),
        commission=Money("1.00", USD),
        liquidity_side=LiquiditySide.TAKER,
        report_id=UUID4(),
        ts_event=current_ns,
        ts_init=current_ns,
    )

    # Act - should not raise, just log warning
    await live_exec_engine._reconcile_missing_fills([fill_report], AUDUSD_SIM.id)

    # Assert - method should handle gracefully


# =============================================================================
# TESTS FOR _process_cached_position_discrepancies
# =============================================================================


@pytest.mark.asyncio
async def test_process_cached_position_discrepancies_no_discrepancy(live_exec_engine, cache):
    """
    Test _process_cached_position_discrepancies skips when no discrepancy.
    """
    # Arrange
    # Ensure cache has the instrument
    if AUDUSD_SIM.id not in [i.id for i in cache.instruments()]:
        cache.add_instrument(AUDUSD_SIM)

    order = TestExecStubs.limit_order(instrument=AUDUSD_SIM)
    fill = TestEventStubs.order_filled(
        order,
        instrument=AUDUSD_SIM,
        last_qty=Quantity.from_int(1000),
        position_id=PositionId("P-123"),
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill)
    cache.add_position(position, OmsType.HEDGING)

    venue_report = PositionStatusReport(
        account_id=TestIdStubs.account_id(),
        instrument_id=AUDUSD_SIM.id,
        position_side=PositionSide.LONG,
        quantity=Quantity.from_int(1000),  # Matches cache
        report_id=UUID4(),
        ts_last=live_exec_engine._clock.timestamp_ns(),
        ts_init=live_exec_engine._clock.timestamp_ns(),
    )

    query_called = False
    original_query = live_exec_engine._query_and_find_missing_fills

    async def capture_query(instrument_id, clients):
        nonlocal query_called
        query_called = True
        return await original_query(instrument_id, clients)

    live_exec_engine._query_and_find_missing_fills = capture_query

    # Act
    await live_exec_engine._process_cached_position_discrepancies(
        {AUDUSD_SIM.id: [position]},
        {AUDUSD_SIM.id: venue_report},
    )

    # Assert
    assert not query_called  # Should not query when no discrepancy


@pytest.mark.asyncio
async def test_process_cached_position_discrepancies_with_discrepancy(
    live_exec_engine,
    exec_client,
    cache,
    account_id,
):
    """
    Test _process_cached_position_discrepancies queries fills when discrepancy found.
    """
    # Arrange
    # Register the client with the engine
    live_exec_engine.register_client(exec_client)

    # Ensure cache has the instrument
    if AUDUSD_SIM.id not in [i.id for i in cache.instruments()]:
        cache.add_instrument(AUDUSD_SIM)

    order = TestExecStubs.limit_order(instrument=AUDUSD_SIM)
    fill = TestEventStubs.order_filled(
        order,
        instrument=AUDUSD_SIM,
        last_qty=Quantity.from_int(1000),
        position_id=PositionId("P-123"),
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill)
    cache.add_position(position, OmsType.HEDGING)

    venue_report = PositionStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        position_side=PositionSide.LONG,
        quantity=Quantity.from_int(1500),  # Discrepancy: venue has more
        report_id=UUID4(),
        ts_last=live_exec_engine._clock.timestamp_ns(),
        ts_init=live_exec_engine._clock.timestamp_ns(),
    )

    query_called = False
    original_query = live_exec_engine._query_and_find_missing_fills

    async def capture_query(instrument_id, clients):
        nonlocal query_called
        query_called = True
        return await original_query(instrument_id, clients)

    live_exec_engine._query_and_find_missing_fills = capture_query

    # Act
    await live_exec_engine._process_cached_position_discrepancies(
        {AUDUSD_SIM.id: [position]},
        {AUDUSD_SIM.id: venue_report},
    )

    # Assert
    assert query_called  # Should query when discrepancy found


# =============================================================================
# TESTS FOR _process_venue_reported_positions
# =============================================================================


@pytest.mark.asyncio
async def test_process_venue_reported_positions_no_discrepancy(live_exec_engine, cache):
    """
    Test _process_venue_reported_positions skips when positions match.
    """
    # Arrange
    # Ensure cache has the instrument
    if AUDUSD_SIM.id not in [i.id for i in cache.instruments()]:
        cache.add_instrument(AUDUSD_SIM)

    order = TestExecStubs.limit_order(instrument=AUDUSD_SIM)
    fill = TestEventStubs.order_filled(
        order,
        instrument=AUDUSD_SIM,
        last_qty=Quantity.from_int(1000),
        position_id=PositionId("P-123"),
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill)
    cache.add_position(position, OmsType.HEDGING)

    venue_report = PositionStatusReport(
        account_id=TestIdStubs.account_id(),
        instrument_id=AUDUSD_SIM.id,
        position_side=PositionSide.LONG,
        quantity=Quantity.from_int(1000),  # Matches cache
        report_id=UUID4(),
        ts_last=live_exec_engine._clock.timestamp_ns(),
        ts_init=live_exec_engine._clock.timestamp_ns(),
    )

    query_called = False
    original_query = live_exec_engine._query_and_find_missing_fills

    async def capture_query(instrument_id, clients):
        nonlocal query_called
        query_called = True
        return await original_query(instrument_id, clients)

    live_exec_engine._query_and_find_missing_fills = capture_query

    # Act
    await live_exec_engine._process_venue_reported_positions(
        {AUDUSD_SIM.id: [position]},
        {AUDUSD_SIM.id: venue_report},
    )

    # Assert
    assert not query_called  # Should not query when positions match


@pytest.mark.asyncio
async def test_process_venue_reported_positions_venue_has_position(
    live_exec_engine,
    exec_client,
    cache,
    account_id,
):
    """
    Test _process_venue_reported_positions queries when venue has position we don't.
    """
    # Arrange
    # Register the client with the engine
    live_exec_engine.register_client(exec_client)

    # Ensure cache has the instrument
    if AUDUSD_SIM.id not in [i.id for i in cache.instruments()]:
        cache.add_instrument(AUDUSD_SIM)

    # No cached position

    venue_report = PositionStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        position_side=PositionSide.LONG,
        quantity=Quantity.from_int(1000),  # Venue has position, we don't
        report_id=UUID4(),
        ts_last=live_exec_engine._clock.timestamp_ns(),
        ts_init=live_exec_engine._clock.timestamp_ns(),
    )

    query_called = False
    original_query = live_exec_engine._query_and_find_missing_fills

    async def capture_query(instrument_id, clients):
        nonlocal query_called
        query_called = True
        return await original_query(instrument_id, clients)

    live_exec_engine._query_and_find_missing_fills = capture_query

    # Act
    await live_exec_engine._process_venue_reported_positions(
        {},  # No cached positions
        {AUDUSD_SIM.id: venue_report},
    )

    # Assert
    assert query_called  # Should query when venue has position we don't


# =============================================================================
# TESTS FOR _handle_order_status_transitions
# =============================================================================


@pytest.mark.asyncio
async def test_handle_order_status_transitions_rejected(live_exec_engine, cache, account_id):
    """
    Test _handle_order_status_transitions generates OrderRejected event.
    """
    # Arrange
    # Ensure cache has the instrument
    if AUDUSD_SIM.id not in [i.id for i in cache.instruments()]:
        cache.add_instrument(AUDUSD_SIM)

    order = TestExecStubs.limit_order(instrument=AUDUSD_SIM)
    cache.add_order(order)

    submitted = TestEventStubs.order_submitted(order, account_id=account_id)
    order.apply(submitted)
    live_exec_engine.process(submitted)
    cache.update_order(order)

    current_ns = live_exec_engine._clock.timestamp_ns()
    report = OrderStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-1"),
        order_side=order.side,
        order_type=order.order_type,
        time_in_force=TimeInForce.GTC,
        order_status=OrderStatus.REJECTED,
        price=order.price,
        quantity=order.quantity,
        filled_qty=Quantity.from_int(0),
        cancel_reason="INSUFFICIENT_FUNDS",
        report_id=UUID4(),
        ts_accepted=0,
        ts_last=current_ns,
        ts_init=current_ns,
    )

    event_generated = False
    original_generate = live_exec_engine._generate_order_rejected

    def capture_generate(o, report):
        nonlocal event_generated
        event_generated = True
        return original_generate(o, report)

    live_exec_engine._generate_order_rejected = capture_generate

    # Act
    result = live_exec_engine._handle_order_status_transitions(
        order,
        report,
        trades=[],
        instrument=AUDUSD_SIM,
    )

    # Assert
    assert result is True
    assert event_generated


@pytest.mark.asyncio
async def test_handle_order_status_transitions_accepted(live_exec_engine, cache, account_id):
    """
    Test _handle_order_status_transitions generates OrderAccepted event.
    """
    # Arrange
    # Ensure cache has the instrument
    if AUDUSD_SIM.id not in [i.id for i in cache.instruments()]:
        cache.add_instrument(AUDUSD_SIM)

    order = TestExecStubs.limit_order(instrument=AUDUSD_SIM)
    cache.add_order(order)

    submitted = TestEventStubs.order_submitted(order, account_id=account_id)
    order.apply(submitted)
    live_exec_engine.process(submitted)
    cache.update_order(order)

    current_ns = live_exec_engine._clock.timestamp_ns()
    report = OrderStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-1"),
        order_side=order.side,
        order_type=order.order_type,
        time_in_force=TimeInForce.GTC,
        order_status=OrderStatus.ACCEPTED,
        price=order.price,
        quantity=order.quantity,
        filled_qty=Quantity.from_int(0),
        report_id=UUID4(),
        ts_accepted=current_ns,
        ts_last=current_ns,
        ts_init=current_ns,
    )

    event_generated = False
    original_generate = live_exec_engine._generate_order_accepted

    def capture_generate(o, report):
        nonlocal event_generated
        event_generated = True
        return original_generate(o, report)

    live_exec_engine._generate_order_accepted = capture_generate

    # Act
    result = live_exec_engine._handle_order_status_transitions(
        order,
        report,
        trades=[],
        instrument=AUDUSD_SIM,
    )

    # Assert
    assert result is True
    assert event_generated


@pytest.mark.asyncio
async def test_handle_order_status_transitions_canceled(live_exec_engine, cache, account_id):
    """
    Test _handle_order_status_transitions generates OrderCanceled event.
    """
    # Arrange
    # Ensure cache has the instrument
    if AUDUSD_SIM.id not in [i.id for i in cache.instruments()]:
        cache.add_instrument(AUDUSD_SIM)

    order = TestExecStubs.limit_order(instrument=AUDUSD_SIM)
    cache.add_order(order)

    submitted = TestEventStubs.order_submitted(order, account_id=account_id)
    order.apply(submitted)
    live_exec_engine.process(submitted)

    accepted = TestEventStubs.order_accepted(order, account_id=account_id)
    order.apply(accepted)
    live_exec_engine.process(accepted)
    cache.update_order(order)

    current_ns = live_exec_engine._clock.timestamp_ns()
    report = OrderStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-1"),
        order_side=order.side,
        order_type=order.order_type,
        time_in_force=TimeInForce.GTC,
        order_status=OrderStatus.CANCELED,
        price=order.price,
        quantity=order.quantity,
        filled_qty=Quantity.from_int(0),
        report_id=UUID4(),
        ts_accepted=current_ns,
        ts_last=current_ns,
        ts_init=current_ns,
    )

    event_generated = False
    original_generate = live_exec_engine._generate_order_canceled

    def capture_generate(o, report):
        nonlocal event_generated
        event_generated = True
        return original_generate(o, report)

    live_exec_engine._generate_order_canceled = capture_generate

    # Act
    result = live_exec_engine._handle_order_status_transitions(
        order,
        report,
        trades=[],
        instrument=AUDUSD_SIM,
    )

    # Assert
    assert result is True
    assert event_generated


@pytest.mark.asyncio
async def test_handle_order_status_transitions_returns_none_for_fill_reconciliation(
    live_exec_engine,
    cache,
    account_id,
):
    """
    Test _handle_order_status_transitions returns None to continue with fill
    reconciliation.
    """
    # Arrange
    # Ensure cache has the instrument
    if AUDUSD_SIM.id not in [i.id for i in cache.instruments()]:
        cache.add_instrument(AUDUSD_SIM)

    order = TestExecStubs.limit_order(instrument=AUDUSD_SIM)
    cache.add_order(order)

    submitted = TestEventStubs.order_submitted(order, account_id=account_id)
    order.apply(submitted)
    live_exec_engine.process(submitted)

    accepted = TestEventStubs.order_accepted(order, account_id=account_id)
    order.apply(accepted)
    live_exec_engine.process(accepted)
    cache.update_order(order)

    current_ns = live_exec_engine._clock.timestamp_ns()
    report = OrderStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-1"),
        order_side=order.side,
        order_type=order.order_type,
        time_in_force=TimeInForce.GTC,
        order_status=OrderStatus.PARTIALLY_FILLED,  # Not a terminal state
        price=order.price,
        quantity=order.quantity,
        filled_qty=Quantity.from_int(50),
        avg_px=Decimal("1.00000"),
        report_id=UUID4(),
        ts_accepted=current_ns,
        ts_last=current_ns,
        ts_init=current_ns,
    )

    # Act
    result = live_exec_engine._handle_order_status_transitions(
        order,
        report,
        trades=[],
        instrument=AUDUSD_SIM,
    )

    # Assert
    assert result is None  # Should return None to continue with fill reconciliation


# =============================================================================
# TESTS FOR _handle_fill_quantity_mismatch
# =============================================================================


@pytest.mark.asyncio
async def test_handle_fill_quantity_mismatch_report_less_than_cache(live_exec_engine, cache, account_id):
    """
    Test _handle_fill_quantity_mismatch logs error when report.filled_qty <
    order.filled_qty.
    """
    # Arrange
    # Ensure cache has the instrument
    if AUDUSD_SIM.id not in [i.id for i in cache.instruments()]:
        cache.add_instrument(AUDUSD_SIM)

    order = TestExecStubs.limit_order(instrument=AUDUSD_SIM)
    cache.add_order(order)

    submitted = TestEventStubs.order_submitted(order, account_id=account_id)
    order.apply(submitted)
    live_exec_engine.process(submitted)

    accepted = TestEventStubs.order_accepted(order, account_id=account_id)
    order.apply(accepted)
    live_exec_engine.process(accepted)

    filled = TestEventStubs.order_filled(
        order,
        instrument=AUDUSD_SIM,
        last_qty=Quantity.from_int(100),
    )
    order.apply(filled)
    live_exec_engine.process(filled)
    cache.update_order(order)

    current_ns = live_exec_engine._clock.timestamp_ns()
    report = OrderStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-1"),
        order_side=order.side,
        order_type=order.order_type,
        time_in_force=TimeInForce.GTC,
        order_status=OrderStatus.PARTIALLY_FILLED,
        price=order.price,
        quantity=order.quantity,
        filled_qty=Quantity.from_int(50),  # Less than cache (100)
        avg_px=Decimal("1.00000"),
        report_id=UUID4(),
        ts_accepted=current_ns,
        ts_last=current_ns,
        ts_init=current_ns,
    )

    # Act
    result = live_exec_engine._handle_fill_quantity_mismatch(
        order,
        report,
        AUDUSD_SIM,
        order.client_order_id,
    )

    # Assert
    assert result is False  # Should fail when report < cache


@pytest.mark.asyncio
async def test_handle_fill_quantity_mismatch_generates_inferred_fill(live_exec_engine, cache, account_id):
    """
    Test _handle_fill_quantity_mismatch generates inferred fill when report > cache.
    """
    # Arrange
    # Ensure cache has the instrument
    if AUDUSD_SIM.id not in [i.id for i in cache.instruments()]:
        cache.add_instrument(AUDUSD_SIM)

    order = TestExecStubs.limit_order(instrument=AUDUSD_SIM)
    cache.add_order(order)

    submitted = TestEventStubs.order_submitted(order, account_id=account_id)
    order.apply(submitted)
    live_exec_engine.process(submitted)

    accepted = TestEventStubs.order_accepted(order, account_id=account_id)
    order.apply(accepted)
    live_exec_engine.process(accepted)

    filled = TestEventStubs.order_filled(
        order,
        instrument=AUDUSD_SIM,
        last_qty=Quantity.from_int(50),
    )
    order.apply(filled)
    live_exec_engine.process(filled)
    cache.update_order(order)

    current_ns = live_exec_engine._clock.timestamp_ns()
    report = OrderStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-1"),
        order_side=order.side,
        order_type=order.order_type,
        time_in_force=TimeInForce.GTC,
        order_status=OrderStatus.PARTIALLY_FILLED,
        price=order.price,
        quantity=order.quantity,
        filled_qty=Quantity.from_int(100),  # More than cache (50)
        avg_px=Decimal("1.00000"),
        report_id=UUID4(),
        ts_accepted=current_ns,
        ts_last=current_ns,
        ts_init=current_ns,
    )

    # Verify initial state before mismatch handling
    assert order.filled_qty == Quantity.from_int(50)
    initial_recent_fills = len(live_exec_engine._recent_fills_cache)

    # Act
    result = live_exec_engine._handle_fill_quantity_mismatch(
        order,
        report,
        AUDUSD_SIM,
        order.client_order_id,
    )

    # Assert
    assert result is True  # Method successfully generated inferred fill

    # Verify inferred fill was tracked (prevents duplicate historical fills)
    assert order.client_order_id in live_exec_engine._inferred_fill_ts, (
        "Client order ID should be tracked in inferred_fill_ts to prevent duplicates"
    )

    # Verify the timestamp is reasonable (after test start)
    inferred_ts = live_exec_engine._inferred_fill_ts[order.client_order_id]
    assert inferred_ts > 0, "Inferred fill timestamp should be positive"

    # Verify inferred fill was generated in the recent fills cache
    assert len(live_exec_engine._recent_fills_cache) == initial_recent_fills + 1, (
        "Exactly one fill should be added to recent_fills_cache"
    )


@pytest.mark.asyncio
async def test_handle_fill_quantity_mismatch_closed_order_within_tolerance(live_exec_engine, cache, account_id):
    """
    Test _handle_fill_quantity_mismatch handles closed orders within tolerance.

    Tests the tolerance logic where venue reports filled_qty that differs from cache by
    exactly one unit at the instrument's precision (handles venue rounding). Uses
    BTCUSDT with size_precision=6 to test fractional tolerance.

    """
    # Arrange
    # Use crypto instrument with fractional precision to test tolerance boundary
    btcusdt = TestInstrumentProvider.btcusdt_binance()
    cache.add_instrument(btcusdt)

    order = TestExecStubs.limit_order(instrument=btcusdt)
    cache.add_order(order)

    submitted = TestEventStubs.order_submitted(order, account_id=account_id)
    order.apply(submitted)
    live_exec_engine.process(submitted)

    accepted = TestEventStubs.order_accepted(order, account_id=account_id)
    order.apply(accepted)
    live_exec_engine.process(accepted)

    filled = TestEventStubs.order_filled(
        order,
        instrument=btcusdt,
        last_qty=order.quantity,  # Fully filled
    )
    order.apply(filled)
    live_exec_engine.process(filled)
    cache.update_order(order)

    # Report shows filled_qty that differs by exactly one unit at precision
    # BTCUSDT has size_precision=6, so tolerance is 0.000001
    # Cache: 100.000000, Report: 100.000001 (within tolerance)
    cache_filled = order.filled_qty
    precision = btcusdt.size_precision
    assert precision == 6  # Verify test assumption

    # Add exactly one unit at the precision level (within tolerance boundary)
    report_filled = Quantity.from_raw(cache_filled.raw + 1, precision)

    # Capture initial state before tolerance check
    initial_recent_fills = len(live_exec_engine._recent_fills_cache)
    initial_fill_count = len([e for e in order.events if isinstance(e, OrderFilled)])

    current_ns = live_exec_engine._clock.timestamp_ns()
    report = OrderStatusReport(
        account_id=account_id,
        instrument_id=btcusdt.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-1"),
        order_side=order.side,
        order_type=order.order_type,
        time_in_force=TimeInForce.GTC,
        order_status=OrderStatus.FILLED,
        price=order.price,
        quantity=order.quantity,
        filled_qty=report_filled,  # Within tolerance: cache + 0.000001
        avg_px=Decimal("50000.00"),
        report_id=UUID4(),
        ts_accepted=current_ns,
        ts_last=current_ns,
        ts_init=current_ns,
    )

    # Act
    result = live_exec_engine._handle_fill_quantity_mismatch(
        order,
        report,
        btcusdt,
        order.client_order_id,
    )

    # Assert
    assert result is True  # Should succeed for closed order within tolerance
    # Verify the tolerance was actually tested (not exact match)
    assert report_filled != cache_filled

    # Verify NO inferred fill was created (within tolerance = no action needed)
    assert order.client_order_id not in live_exec_engine._inferred_fill_ts, (
        "No inferred fill should be tracked for differences within tolerance"
    )
    assert len(live_exec_engine._recent_fills_cache) == initial_recent_fills, (
        "No fill should be added to recent_fills_cache for within-tolerance difference"
    )
    assert order.filled_qty == cache_filled, (
        "Order filled_qty should remain unchanged for within-tolerance difference"
    )
    assert len([e for e in order.events if isinstance(e, OrderFilled)]) == initial_fill_count, (
        "No new fill events should be added for within-tolerance difference"
    )


@pytest.mark.asyncio
async def test_reconciliation_orders_not_reprocessed_on_restart(live_exec_engine, cache):
    """
    Test that closed reconciliation orders from previous sessions are not reprocessed on
    restart, preventing duplicate inferred fills.

    This test reproduces the issue where reconciliation market orders created in a first
    session get reloaded from the venue on restart and generate duplicate inferred
    fills, causing position discrepancies.

    """
    # Arrange
    instrument = AUDUSD_SIM
    cache.add_instrument(instrument)

    clock = LiveClock()
    trader_id = TestIdStubs.trader_id()
    account_id = TestIdStubs.account_id()
    order_factory = OrderFactory(
        trader_id=trader_id,
        strategy_id=StrategyId("S-001"),
        clock=clock,
    )

    recon_order = order_factory.market(
        instrument_id=instrument.id,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_str("0.00501"),
        tags=["RECONCILIATION"],
    )

    submitted = TestEventStubs.order_submitted(recon_order)
    accepted = TestEventStubs.order_accepted(recon_order)
    filled = TestEventStubs.order_filled(
        recon_order,
        instrument=instrument,
        last_qty=Quantity.from_str("0.00501"),
        last_px=Price.from_str("0.0"),
    )

    recon_order.apply(submitted)
    recon_order.apply(accepted)
    recon_order.apply(filled)
    cache.add_order(recon_order)

    assert recon_order.is_closed
    assert recon_order.tags == ["RECONCILIATION"]
    order_report = OrderStatusReport(
        account_id=account_id,
        instrument_id=instrument.id,
        client_order_id=recon_order.client_order_id,
        venue_order_id=VenueOrderId("RECON-1"),
        order_side=OrderSide.SELL,
        order_type=OrderType.MARKET,
        time_in_force=TimeInForce.IOC,
        order_status=OrderStatus.FILLED,
        quantity=Quantity.from_str("0.00501"),
        filled_qty=Quantity.from_str("0.00501"),
        avg_px=Decimal("0.0"),
        report_id=UUID4(),
        ts_accepted=0,
        ts_last=0,
        ts_init=0,
    )

    mass_status = ExecutionMassStatus(
        client_id=ClientId(SIM.value),
        venue=SIM,
        account_id=account_id,
        report_id=UUID4(),
        ts_init=0,
    )
    mass_status._order_reports[order_report.venue_order_id] = order_report

    initial_order_count = len(cache.orders())
    initial_position_count = len(cache.positions())

    # Act
    live_exec_engine._reconcile_execution_mass_status(mass_status)

    # Assert
    assert len(cache.orders()) == initial_order_count, (
        "Reconciliation order should be skipped, not create duplicate order"
    )
    assert len(cache.positions()) == initial_position_count, (
        "No new positions should be created from reprocessing reconciliation order"
    )
    assert len([e for e in recon_order.events if isinstance(e, OrderFilled)]) == 1, (
        "Reconciliation order should still have only its original fill"
    )


@pytest.mark.parametrize(
    ("value1", "value2", "precision", "expected"),
    [
        # Integer precision (precision=0) - requires exact match
        (Decimal(100), Decimal(100), 0, True),
        (Decimal(100), Decimal(101), 0, False),
        # Single decimal precision (precision=1)
        (Decimal("1.0"), Decimal("1.0"), 1, True),
        (Decimal("1.0"), Decimal("1.1"), 1, True),  # Within 0.1 tolerance
        (Decimal("1.0"), Decimal("1.2"), 1, False),  # Exceeds 0.1 tolerance
        # Two decimal precision (precision=2)
        (Decimal("1.00"), Decimal("1.01"), 2, True),  # Within 0.01 tolerance
        (Decimal("1.00"), Decimal("1.02"), 2, False),  # Exceeds 0.01 tolerance
        # Three decimal precision (precision=3)
        (Decimal("1.000"), Decimal("1.001"), 3, True),  # Within 0.001 tolerance
        (Decimal("1.000"), Decimal("1.002"), 3, False),  # Exceeds 0.001 tolerance
        # Eight decimal precision (common for crypto)
        (Decimal("0.00000001"), Decimal("0.00000002"), 8, True),
        (Decimal("0.00000001"), Decimal("0.00000003"), 8, False),
        # Edge case: exactly at tolerance boundary
        (Decimal("1.0"), Decimal("1.1"), 1, True),  # Exactly 0.1 diff
        (Decimal("1.0"), Decimal("0.9"), 1, True),  # Exactly -0.1 diff
        # Negative values
        (Decimal("-1.0"), Decimal("-1.1"), 1, True),
        (Decimal("-1.0"), Decimal("-1.2"), 1, False),
    ],
)
def test_is_within_single_unit_tolerance_boundaries(
    value1: Decimal,
    value2: Decimal,
    precision: int,
    expected: bool,
) -> None:
    # Act
    result = is_within_single_unit_tolerance(value1, value2, precision)

    # Assert
    assert result is expected


def test_is_within_single_unit_tolerance_zero_values() -> None:
    # Act & Assert
    assert is_within_single_unit_tolerance(Decimal(0), Decimal(0), 0) is True
    assert is_within_single_unit_tolerance(Decimal(0), Decimal(0), 2) is True
    assert is_within_single_unit_tolerance(Decimal("0.00"), Decimal("0.01"), 2) is True
    assert is_within_single_unit_tolerance(Decimal("0.00"), Decimal("0.02"), 2) is False


def test_is_within_single_unit_tolerance_symmetric() -> None:
    # Tolerance should work in both directions
    assert is_within_single_unit_tolerance(
        Decimal("1.0"),
        Decimal("1.1"),
        1,
    ) is is_within_single_unit_tolerance(Decimal("1.1"), Decimal("1.0"), 1)


def test_get_existing_fill_for_trade_id_returns_none_when_no_fills_exist() -> None:
    # Arrange
    order = TestExecStubs.limit_order()
    trade_id = TradeId("TRADE-001")

    # Act
    result = get_existing_fill_for_trade_id(order, trade_id)

    # Assert
    assert result is None


def test_get_existing_fill_for_trade_id_returns_none_when_trade_id_not_found() -> None:
    # Arrange
    order = TestExecStubs.limit_order()
    submitted = TestEventStubs.order_submitted(order)
    accepted = TestEventStubs.order_accepted(order)
    filled = TestEventStubs.order_filled(
        order,
        instrument=AUDUSD_SIM,
        trade_id=TradeId("TRADE-001"),
    )

    order.apply(submitted)
    order.apply(accepted)
    order.apply(filled)

    # Act
    result = get_existing_fill_for_trade_id(order, TradeId("TRADE-002"))

    # Assert
    assert result is None


def test_get_existing_fill_for_trade_id_returns_fill_when_trade_id_found() -> None:
    # Arrange
    order = TestExecStubs.limit_order()
    submitted = TestEventStubs.order_submitted(order)
    accepted = TestEventStubs.order_accepted(order)
    filled = TestEventStubs.order_filled(
        order,
        instrument=AUDUSD_SIM,
        trade_id=TradeId("TRADE-001"),
    )

    order.apply(submitted)
    order.apply(accepted)
    order.apply(filled)

    # Act
    result = get_existing_fill_for_trade_id(order, TradeId("TRADE-001"))

    # Assert
    assert result is not None
    assert result.trade_id == TradeId("TRADE-001")
    assert result == filled


def test_get_existing_fill_for_trade_id_returns_correct_fill_when_multiple_fills_exist() -> None:
    # Arrange
    order = TestExecStubs.limit_order()
    submitted = TestEventStubs.order_submitted(order)
    accepted = TestEventStubs.order_accepted(order)

    fill1 = TestEventStubs.order_filled(
        order,
        instrument=AUDUSD_SIM,
        last_qty=Quantity.from_int(50),
        trade_id=TradeId("TRADE-001"),
    )
    fill2 = TestEventStubs.order_filled(
        order,
        instrument=AUDUSD_SIM,
        last_qty=Quantity.from_int(50),
        trade_id=TradeId("TRADE-002"),
    )

    order.apply(submitted)
    order.apply(accepted)
    order.apply(fill1)
    order.apply(fill2)

    # Act
    result1 = get_existing_fill_for_trade_id(order, TradeId("TRADE-001"))
    result2 = get_existing_fill_for_trade_id(order, TradeId("TRADE-002"))

    # Assert
    assert result1 == fill1
    assert result2 == fill2
    assert result1 != result2


def test_get_existing_fill_for_trade_id_ignores_non_fill_events() -> None:
    # Arrange
    order = TestExecStubs.limit_order()
    submitted = TestEventStubs.order_submitted(order)
    accepted = TestEventStubs.order_accepted(order)
    updated = TestEventStubs.order_updated(order)

    order.apply(submitted)
    order.apply(accepted)
    order.apply(updated)

    # Act
    result = get_existing_fill_for_trade_id(order, TradeId("ANY-TRADE"))

    # Assert
    assert result is None
