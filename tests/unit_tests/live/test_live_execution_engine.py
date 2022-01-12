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

import pytest

from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.execution.messages import ExecutionReport
from nautilus_trader.execution.messages import OrderStatusReport
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_engine import LiveExecEngineConfig
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.risk_engine import LiveRiskEngine
from nautilus_trader.model.commands.trading import SubmitOrder
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
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


class TestLiveExecutionEngine:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(self.clock)

        self.trader_id = TestStubs.trader_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            clock=self.clock,
        )

        self.random_order_factory = OrderFactory(
            trader_id=TraderId("RANDOM-042"),
            strategy_id=StrategyId("S-042"),
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

        self.instrument_provider = InstrumentProvider()
        self.instrument_provider.add(AUDUSD_SIM)
        self.instrument_provider.add(GBPUSD_SIM)

        self.client = MockLiveExecutionClient(
            loop=self.loop,
            client_id=ClientId(SIM.value),
            account_type=AccountType.CASH,
            base_currency=USD,
            instrument_provider=self.instrument_provider,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )
        self.portfolio.update_account(TestStubs.event_cash_account_state())
        self.exec_engine.register_client(self.client)

        self.cache.add_instrument(AUDUSD_SIM)

    def teardown(self):
        self.exec_engine.dispose()

    @pytest.mark.asyncio
    async def test_start_when_loop_not_running_logs(self):
        # Arrange, Act
        self.exec_engine.start()

        # Assert
        assert True  # No exceptions raised
        self.exec_engine.stop()

    @pytest.mark.asyncio
    async def test_message_qsize_at_max_blocks_on_put_command(self):
        # Arrange
        # Deregister test fixture ExecutionEngine from msgbus)
        self.msgbus.deregister(endpoint="ExecEngine.execute", handler=self.exec_engine.execute)
        self.msgbus.deregister(endpoint="ExecEngine.process", handler=self.exec_engine.process)

        self.exec_engine = LiveExecutionEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=LiveExecEngineConfig(qsize=1),
        )

        strategy = TradingStrategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            None,
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.execute(submit_order)
        self.exec_engine.execute(submit_order)
        await asyncio.sleep(0.1)

        # Assert
        assert self.exec_engine.qsize() == 1
        assert self.exec_engine.command_count == 0

    @pytest.mark.asyncio
    async def test_message_qsize_at_max_blocks_on_put_event(self):
        # Arrange
        # Deregister test fixture ExecutionEngine from msgbus)
        self.msgbus.deregister(endpoint="ExecEngine.execute", handler=self.exec_engine.execute)
        self.msgbus.deregister(endpoint="ExecEngine.process", handler=self.exec_engine.process)

        self.exec_engine = LiveExecutionEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=LiveExecEngineConfig(qsize=1),
        )

        strategy = TradingStrategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            None,
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        event = TestStubs.event_order_submitted(order)

        # Act
        self.exec_engine.execute(submit_order)
        self.exec_engine.process(event)  # Add over max size
        await asyncio.sleep(0.1)

        # Assert
        assert self.exec_engine.qsize() == 1
        assert self.exec_engine.command_count == 0

    @pytest.mark.asyncio
    async def test_start(self):
        # Arrange, Act
        self.exec_engine.start()
        await asyncio.sleep(0.1)

        # Assert
        assert self.exec_engine.is_running

        # Tear Down
        self.exec_engine.stop()

    @pytest.mark.asyncio
    async def test_kill_when_running_and_no_messages_on_queues(self):
        # Arrange, Act
        self.exec_engine.start()
        await asyncio.sleep(0)
        self.exec_engine.kill()

        # Assert
        assert self.exec_engine.is_stopped

    @pytest.mark.asyncio
    async def test_kill_when_not_running_with_messages_on_queue(self):
        # Arrange, Act
        self.exec_engine.kill()

        # Assert
        assert self.exec_engine.qsize() == 0

    @pytest.mark.asyncio
    async def test_execute_command_places_command_on_queue(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            None,
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.execute(submit_order)
        await asyncio.sleep(0.1)

        # Assert
        assert self.exec_engine.qsize() == 0
        assert self.exec_engine.command_count == 1

        # Tear Down
        self.exec_engine.stop()

    @pytest.mark.asyncio
    async def test_reconcile_state_with_no_active_orders(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        await self.exec_engine.reconcile_state(timeout_secs=10)
        self.exec_engine.stop()
        await asyncio.sleep(0.1)

        # Assert
        assert True  # No exceptions raised

    @pytest.mark.asyncio
    async def test_reconcile_state_when_report_agrees_reconciles(self):
        # Arrange
        self.exec_engine.start()

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

        self.exec_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))

        report = OrderStatusReport(
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("1"),  # <-- from stub event
            order_status=OrderStatus.ACCEPTED,
            filled_qty=Quantity.zero(),
            ts_init=0,
        )

        self.client.add_order_status_report(report)

        await asyncio.sleep(0.1)  # Allow processing time

        # Act
        result = await self.exec_engine.reconcile_state(timeout_secs=10)
        self.exec_engine.stop()

        # Assert
        assert result

    @pytest.mark.asyncio
    async def test_reconcile_state_when_canceled_reconciles(self):
        # Arrange
        self.exec_engine.start()

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

        self.exec_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))

        report = OrderStatusReport(
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("1"),  # <-- from stub event
            order_status=OrderStatus.CANCELED,
            filled_qty=Quantity.zero(),
            ts_init=0,
        )

        self.client.add_order_status_report(report)

        await asyncio.sleep(0.1)  # Allow processing time

        # Act
        result = await self.exec_engine.reconcile_state(timeout_secs=10)
        self.exec_engine.stop()

        # Assert
        assert result

    @pytest.mark.asyncio
    async def test_reconcile_state_when_expired_reconciles(self):
        # Arrange
        self.exec_engine.start()

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

        self.exec_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))

        report = OrderStatusReport(
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("1"),  # <-- from stub event
            order_status=OrderStatus.EXPIRED,
            filled_qty=Quantity.zero(),
            ts_init=0,
        )

        self.client.add_order_status_report(report)

        await asyncio.sleep(0.1)  # Allow processing time

        # Act
        result = await self.exec_engine.reconcile_state(timeout_secs=10)
        self.exec_engine.stop()

        # Assert
        assert result

    @pytest.mark.skip(reason="reimplement reconciliation")
    @pytest.mark.asyncio
    async def test_reconcile_state_when_partially_filled_reconciles(self):
        # Arrange
        self.exec_engine.start()

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

        self.exec_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))

        report = OrderStatusReport(
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("1"),  # <-- from stub event
            order_status=OrderStatus.PARTIALLY_FILLED,
            filled_qty=Quantity.from_int(70000),
            ts_init=0,
        )

        trade1 = ExecutionReport(
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("1"),
            venue_position_id=None,
            execution_id=ExecutionId("1"),
            last_qty=Quantity.from_int(50000),
            last_px=Price.from_str("1.00000"),
            commission=Money(5.00, USD),
            liquidity_side=LiquiditySide.MAKER,
            ts_event=0,
            ts_init=0,
        )

        trade2 = ExecutionReport(
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("1"),
            venue_position_id=None,
            execution_id=ExecutionId("2"),
            last_qty=Quantity.from_int(20000),
            last_px=Price.from_str("1.00000"),
            commission=Money(2.00, USD),
            liquidity_side=LiquiditySide.MAKER,
            ts_event=0,
            ts_init=0,
        )

        self.client.add_order_status_report(report)
        self.client.add_trades_list(VenueOrderId("1"), [trade1, trade2])

        await asyncio.sleep(0.1)  # Allow processing time

        # Act
        result = await self.exec_engine.reconcile_state(timeout_secs=10)
        self.exec_engine.stop()

        # Assert
        assert result

    @pytest.mark.skip(reason="reimplement reconciliation")
    @pytest.mark.asyncio
    async def test_reconcile_state_when_filled_reconciles(self):
        # Arrange
        self.exec_engine.start()

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

        self.exec_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))

        report = OrderStatusReport(
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("1"),  # <-- from stub event
            order_status=OrderStatus.FILLED,
            filled_qty=Quantity.from_int(100000),
            ts_init=0,
        )

        trade1 = ExecutionReport(
            execution_id=ExecutionId("1"),
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("1"),
            venue_position_id=None,
            last_qty=Quantity.from_int(50000),
            last_px=Price.from_str("1.00000"),
            commission=Money(5.00, USD),
            liquidity_side=LiquiditySide.MAKER,
            ts_event=0,
            ts_init=0,
        )

        trade2 = ExecutionReport(
            execution_id=ExecutionId("2"),
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("1"),
            venue_position_id=None,
            last_qty=Quantity.from_int(50000),
            last_px=Price.from_str("1.00000"),
            commission=Money(2.00, USD),
            liquidity_side=LiquiditySide.MAKER,
            ts_event=0,
            ts_init=0,
        )

        self.client.add_order_status_report(report)
        self.client.add_trades_list(VenueOrderId("1"), [trade1, trade2])

        await asyncio.sleep(0.1)  # Allow processing time

        # Act
        result = await self.exec_engine.reconcile_state(timeout_secs=10)
        self.exec_engine.stop()

        # Assert
        assert result
