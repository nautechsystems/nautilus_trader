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
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.risk_engine import LiveRiskEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
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
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.functions import ensure_all_tasks_completed
from nautilus_trader.test_kit.mocks.exec_clients import MockLiveExecutionClient
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
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
        result = await self.exec_engine.reconcile_state()

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
        result = await self.exec_engine.reconcile_state()

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
        result = await self.exec_engine.reconcile_state()

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
        result = await self.exec_engine.reconcile_state()

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
        result = await self.exec_engine.reconcile_state()

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
        result = await self.exec_engine.reconcile_state()

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
        result = await self.exec_engine.reconcile_state()

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
        result = await self.exec_engine.reconcile_state()

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
        result = await self.exec_engine.reconcile_state()

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
        result = await self.exec_engine.reconcile_state()

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
        result = await self.exec_engine.reconcile_state()

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
