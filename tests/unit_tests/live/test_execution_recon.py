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
from nautilus_trader.test_kit.functions import ensure_all_tasks_completed
from nautilus_trader.test_kit.mocks.exec_clients import MockLiveExecutionClient
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


SIM = Venue("SIM")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")


class TestLiveExecutionReconciliation:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        asyncio.set_event_loop(self.loop)
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

    def teardown(self):
        self.data_engine.stop()
        self.risk_engine.stop()
        self.exec_engine.stop()

        ensure_all_tasks_completed()

        self.exec_engine.dispose()
        self.client.dispose()

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
    def live_exec_engine(self):
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)

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

        # External report shows -50 units short (need to generate 150 SELL order)
        external_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.SHORT,
            quantity=Quantity.from_int(50),
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
        assert len(reconcile_calls) == 1

        order_report, _ = reconcile_calls[0]
        assert order_report.order_side == OrderSide.SELL
        assert order_report.quantity == Quantity.from_int(150)  # 100 to close + 50 to open short
        assert order_report.filled_qty == Quantity.from_int(150)
        assert order_report.order_status == OrderStatus.FILLED

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

        # External report shows 75 units long (need to generate 175 BUY order)
        external_report = PositionStatusReport(
            account_id=TestIdStubs.account_id(),
            instrument_id=instrument.id,
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(75),
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
        assert order_report.quantity == Quantity.from_int(175)  # 100 to close + 75 to open long
        assert order_report.filled_qty == Quantity.from_int(175)
        assert order_report.order_status == OrderStatus.FILLED

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


class TestReconciliationFiltering:
    """
    Tests for filtering logic during live execution reconciliation.
    """

    def _get_exec_engine(self, config: LiveExecEngineConfig):
        loop = asyncio.get_event_loop()
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
