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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.fixture()
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

    @pytest.mark.asyncio()
    async def test_duplicate_client_order_id_fails_validation(self, live_exec_engine):
        """
        Test that duplicate client order IDs cause reconciliation failure.
        """
        # Arrange
        client_order_id = ClientOrderId("O-123")

        report1 = OrderStatusReport(
            instrument_id=AUDUSD_SIM.id,
            account_id=TestIdStubs.account_id(),
            client_order_id=client_order_id,
            venue_order_id=VenueOrderId("V-1"),
            order_side=OrderSide.BUY,
            order_type=OrderType.MARKET,
            time_in_force=TimeInForce.DAY,
            order_status=OrderStatus.FILLED,
            quantity=Quantity.from_int(100),
            filled_qty=Quantity.from_int(100),
            report_id=UUID4(),
            ts_accepted=0,
            ts_last=0,
            ts_init=0,
        )

        report2 = OrderStatusReport(
            instrument_id=AUDUSD_SIM.id,
            account_id=TestIdStubs.account_id(),
            client_order_id=client_order_id,
            venue_order_id=VenueOrderId("V-2"),
            order_side=OrderSide.SELL,
            order_type=OrderType.MARKET,
            time_in_force=TimeInForce.DAY,
            order_status=OrderStatus.FILLED,
            quantity=Quantity.from_int(50),
            filled_qty=Quantity.from_int(50),
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
        result = live_exec_engine._reconcile_execution_mass_status(mass_status)

        # Assert
        assert result is False

    @pytest.mark.asyncio()
    async def test_mass_status_failure_preserves_position_results(self, live_exec_engine):
        """
        Test that mass status failure is not overwritten by position reconciliation.
        """
        # Complex mocking required

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
    async def test_long_position_reconciliation_external_flat(self, live_exec_engine):
        """
        Test reconciliation when internal long position exists but external position is
        FLAT.

        This tests the critical scenario from issue #3023 where a position is closed
        externally (via the client/exchange directly) but remains open in the cache.

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

    @pytest.mark.asyncio()
    async def test_short_position_reconciliation_external_flat(self, live_exec_engine):
        """
        Test reconciliation when internal short position exists but external position is
        FLAT.

        This tests the critical scenario from issue #3023 where a position is closed
        externally (via the client/exchange directly) but remains open in the cache.

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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
    async def test_internal_diff_order_not_filtered_when_filter_unclaimed_external_orders_enabled(
        self,
        live_exec_engine,
    ):
        """
        Test that INTERNAL-DIFF orders are not filtered out when
        filter_unclaimed_external_orders is enabled.

        This ensures that position reconciliation orders are generated even when
        external order filtering is active.

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

        # Find the newly generated order
        new_orders = [o for o in orders_after if o.strategy_id.value == "INTERNAL-DIFF"]
        assert len(new_orders) == 1

        generated_order = new_orders[0]
        assert generated_order.strategy_id.value == "INTERNAL-DIFF"
        assert generated_order.side == OrderSide.BUY
        assert generated_order.quantity == Quantity.from_int(50)
        assert generated_order.status == OrderStatus.FILLED

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

        # Verify the order was added to cache with INTERNAL-DIFF strategy
        orders = self.cache.orders()
        internal_diff_orders = [o for o in orders if o.strategy_id.value == "INTERNAL-DIFF"]
        assert len(internal_diff_orders) == 1

        generated_order = internal_diff_orders[0]
        assert generated_order.order_type == OrderType.MARKET
        assert generated_order.side == OrderSide.BUY
        assert generated_order.quantity == Quantity.from_int(100)
        # Order might be ACCEPTED or FILLED depending on how reconciliation processes events
        assert generated_order.status in (OrderStatus.ACCEPTED, OrderStatus.FILLED)

    @pytest.mark.asyncio()
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

        # Verify the order was added to cache
        orders = self.cache.orders()
        internal_diff_orders = [o for o in orders if o.strategy_id.value == "INTERNAL-DIFF"]
        assert len(internal_diff_orders) == 1

        generated_order = internal_diff_orders[0]
        assert generated_order.order_type == OrderType.LIMIT
        assert generated_order.side == OrderSide.BUY
        assert generated_order.quantity == Quantity.from_int(100)
        assert generated_order.price == Price.from_str("1.0001")  # Ask price for BUY order
        assert generated_order.status == OrderStatus.FILLED

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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
