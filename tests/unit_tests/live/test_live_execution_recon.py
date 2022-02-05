# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.risk_engine import LiveRiskEngine
from nautilus_trader.model.commands.trading import SubmitOrder
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.mocks import MockLiveExecutionClient
from tests.test_kit.stubs import TestStubs


SIM = Venue("SIM")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")


class TestLiveExecutionReconciliation:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.logger = LiveLogger(self.loop, self.clock)

        self.account_id = TestStubs.account_id()
        self.trader_id = TestStubs.trader_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            clock=self.clock,
        )

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine = LiveDataEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_engine = LiveExecutionEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.risk_engine = LiveRiskEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.client = MockLiveExecutionClient(
            loop=self.loop,
            client_id=ClientId(SIM.value),
            account_type=AccountType.CASH,
            base_currency=USD,
            instrument_provider=InstrumentProvider(),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )
        self.portfolio.update_account(TestStubs.event_cash_account_state())
        self.exec_engine.register_client(self.client)

        # Prepare components
        self.cache.add_instrument(AUDUSD_SIM)

    def teardown(self):
        self.client.dispose()

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
        result = await self.exec_engine.reconcile_state()

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
        result = await self.exec_engine.reconcile_state()

        # Assert
        assert result
        assert len(self.cache.orders()) == 1
        assert len(self.cache.orders_working()) == 1
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
            expire_time=None,
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

    @pytest.mark.asyncio
    async def test_reconcile_state_no_cached_with_expired_order(self):
        # Arrange
        report = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            order_list_id=None,
            venue_order_id=VenueOrderId("1"),
            order_side=OrderSide.BUY,
            order_type=OrderType.STOP_LIMIT,
            time_in_force=TimeInForce.GTD,
            expire_time=pd.Timestamp("1970-01-01T00:10:00", tz="UTC"),
            order_status=OrderStatus.EXPIRED,
            price=Price.from_str("0.99500"),
            trigger_price=Price.from_str("1.00000"),
            trigger_type=TriggerType.BID_ASK,
            offset_type=TrailingOffsetType.PRICE,
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
            offset_type=TrailingOffsetType.PRICE,
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
        assert len(self.cache.orders_working()) == 1
        assert self.cache.orders()[0].status == OrderStatus.TRIGGERED

    @pytest.mark.asyncio
    async def test_reconcile_state_no_cached_with_filled_order_and_no_trades(self):
        # Arrange
        report = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            order_list_id=None,
            venue_order_id=VenueOrderId("1"),
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            contingency_type=ContingencyType.NONE,
            time_in_force=TimeInForce.GTC,
            expire_time=None,
            order_status=OrderStatus.FILLED,
            price=Price.from_str("1.00000"),
            trigger_price=None,
            trigger_type=TriggerType.NONE,
            limit_offset=None,
            trailing_offset=None,
            offset_type=TrailingOffsetType.NONE,
            quantity=Quantity.from_int(10_000),
            filled_qty=Quantity.from_int(10_000),
            display_qty=None,
            avg_px=Decimal("1.00000"),
            post_only=True,
            reduce_only=False,
            cancel_reason=None,
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

    @pytest.mark.asyncio
    async def test_reconcile_state_no_cached_with_filled_order_and_trade(self):
        # Arrange
        venue_order_id = VenueOrderId("1")
        order_report = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            order_list_id=None,
            venue_order_id=venue_order_id,
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            contingency_type=ContingencyType.NONE,
            time_in_force=TimeInForce.GTC,
            expire_time=None,
            order_status=OrderStatus.FILLED,
            price=Price.from_str("1.00000"),
            trigger_price=None,
            trigger_type=TriggerType.NONE,
            limit_offset=None,
            trailing_offset=None,
            offset_type=TrailingOffsetType.NONE,
            quantity=Quantity.from_int(10_000),
            filled_qty=Quantity.from_int(10_000),
            display_qty=None,
            avg_px=Decimal("1.00000"),
            post_only=True,
            reduce_only=False,
            cancel_reason=None,
            report_id=UUID4(),
            ts_accepted=0,
            ts_triggered=0,
            ts_last=0,
            ts_init=0,
        )

        trade_report = TradeReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=venue_order_id,
            venue_position_id=None,
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
        self.client.add_trade_reports(venue_order_id, [trade_report])

        # Act
        result = await self.exec_engine.reconcile_state()

        # Assert
        assert result
        assert len(self.cache.orders()) == 1
        assert self.cache.orders()[0].status == OrderStatus.FILLED

    @pytest.mark.skip(reason="WIP")
    @pytest.mark.asyncio
    async def test_reconcile_state_when_order_rejected_generates_event(self):
        # Arrange
        self.exec_engine.start()
        self.risk_engine.start()

        strategy = TradingStrategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            None,
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        await asyncio.sleep(0)  # Process queue
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        await asyncio.sleep(0)  # Process queue
        self.exec_engine.process(TestStubs.event_order_accepted(order))
        await asyncio.sleep(0)  # Process queue

        report = OrderStatusReport(
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("1"),  # <-- from stub event
            order_status=OrderStatus.REJECTED,
            filled_qty=Quantity.zero(),
            ts_init=0,
        )

        # Act
        result = self.client.reconcile_state(report, order)

        # Assert
        assert result

    @pytest.mark.skip(reason="WIP")
    @pytest.mark.asyncio
    async def test_reconcile_state_when_order_expired_generates_event(self):
        # Arrange
        self.exec_engine.start()
        self.risk_engine.start()

        strategy = TradingStrategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            None,
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        await asyncio.sleep(0)  # Process queue
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        await asyncio.sleep(0)  # Process queue
        self.exec_engine.process(TestStubs.event_order_accepted(order))
        await asyncio.sleep(0)  # Process queue

        report = OrderStatusReport(
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("1"),  # <-- from stub event
            order_status=OrderStatus.EXPIRED,
            filled_qty=Quantity.zero(),
            ts_init=0,
        )

        # Act
        result = self.client.reconcile_state(report, order)

        # Assert
        assert result

    @pytest.mark.skip(reason="WIP")
    @pytest.mark.asyncio
    async def test_reconcile_state_when_order_canceled_generates_event(self):
        # Arrange
        self.exec_engine.start()
        self.risk_engine.start()

        strategy = TradingStrategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            None,
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        await asyncio.sleep(0)  # Process queue
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        await asyncio.sleep(0)  # Process queue
        self.exec_engine.process(TestStubs.event_order_accepted(order))
        await asyncio.sleep(0)  # Process queue

        report = OrderStatusReport(
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("1"),  # <-- from stub event
            order_status=OrderStatus.CANCELED,
            filled_qty=Quantity.zero(),
            ts_init=0,
        )

        # Act
        result = self.client.reconcile_state(report, order)

        # Assert
        assert result

    @pytest.mark.skip(reason="WIP")
    @pytest.mark.asyncio
    async def test_reconcile_state_when_order_completed_returns_true_with_warning1(self):
        # Arrange
        self.exec_engine.start()
        self.risk_engine.start()

        strategy = TradingStrategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            None,
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        await asyncio.sleep(0)  # Process queue
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        await asyncio.sleep(0)  # Process queue
        self.exec_engine.process(TestStubs.event_order_accepted(order))
        await asyncio.sleep(0)  # Process queue
        self.exec_engine.process(TestStubs.event_order_canceled(order))
        await asyncio.sleep(0)  # Process queue

        report = OrderStatusReport(
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("1"),  # <-- from stub event
            order_status=OrderStatus.CANCELED,
            filled_qty=Quantity.zero(),
            ts_init=0,
        )

        # Act
        result = await self.client.reconcile_state(report, order)

        # Assert
        assert result

    @pytest.mark.skip(reason="WIP")
    @pytest.mark.asyncio
    async def test_reconcile_state_when_order_completed_returns_true_with_warning2(self):
        # Arrange
        self.exec_engine.start()
        self.risk_engine.start()

        strategy = TradingStrategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            None,
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        await asyncio.sleep(0)  # Process queue
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        await asyncio.sleep(0)  # Process queue
        self.exec_engine.process(TestStubs.event_order_accepted(order))
        await asyncio.sleep(0)  # Process queue
        self.exec_engine.process(TestStubs.event_order_filled(order, AUDUSD_SIM))
        await asyncio.sleep(0)  # Process queue

        report = OrderStatusReport(
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("1"),  # <-- from stub event
            order_status=OrderStatus.FILLED,
            filled_qty=Quantity.from_int(100000),
            ts_init=0,
        )

        # Act
        result = await self.client.reconcile_state(report, order)

        # Assert
        assert result

    @pytest.mark.skip(reason="WIP")
    @pytest.mark.asyncio
    async def test_reconcile_state_with_filled_order_when_trades_not_given_returns_false(
        self,
    ):
        # Arrange
        self.exec_engine.start()
        self.risk_engine.start()

        strategy = TradingStrategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            None,
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        await asyncio.sleep(0)  # Process queue
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        await asyncio.sleep(0)  # Process queue
        self.exec_engine.process(TestStubs.event_order_accepted(order))
        await asyncio.sleep(0)  # Process queue

        report = OrderStatusReport(
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("1"),  # <-- from stub event
            order_status=OrderStatus.FILLED,
            filled_qty=Quantity.from_int(100000),
            ts_init=0,
        )

        # Act
        result = await self.client.reconcile_state(report, order)

        # Assert
        assert not result
