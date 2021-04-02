# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.analysis.performance import PerformanceAnalyzer
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.enums import ComponentState
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.data.cache import DataCache
from nautilus_trader.execution.database import BypassExecutionDatabase
from nautilus_trader.execution.messages import ExecutionReport
from nautilus_trader.execution.messages import OrderStatusReport
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderState
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import OrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.mocks import MockLiveExecutionClient
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


SIM = Venue("SIM")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")


class TestLiveExecutionEngine:
    def setup(self):
        # Fixture Setup
        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(self.clock)

        self.trader_id = TraderId("TESTER", "000")
        self.account_id = TestStubs.account_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S", "001"),
            clock=self.clock,
        )

        self.random_order_factory = OrderFactory(
            trader_id=TraderId("RANDOM", "042"),
            strategy_id=StrategyId("S", "042"),
            clock=self.clock,
        )

        self.portfolio = Portfolio(
            clock=self.clock,
            logger=self.logger,
        )
        self.portfolio.register_cache(DataCache(self.logger))

        self.analyzer = PerformanceAnalyzer()

        # Fresh isolated loop testing pattern
        self.loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self.loop)

        self.database = BypassExecutionDatabase(
            trader_id=self.trader_id, logger=self.logger
        )
        self.engine = LiveExecutionEngine(
            loop=self.loop,
            database=self.database,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
        )

        self.instrument_provider = InstrumentProvider()
        self.instrument_provider.add(AUDUSD_SIM)
        self.instrument_provider.add(GBPUSD_SIM)

        self.client = MockLiveExecutionClient(
            name=SIM.value,
            account_id=self.account_id,
            engine=self.engine,
            instrument_provider=self.instrument_provider,
            clock=self.clock,
            logger=self.logger,
        )

        self.engine.register_client(self.client)

    def teardown(self):
        self.engine.dispose()
        self.loop.stop()
        self.loop.close()

    def test_start_when_loop_not_running_logs(self):
        # Arrange
        # Act
        self.engine.start()

        # Assert
        assert True  # No exceptions raised
        self.engine.stop()

    def test_get_event_loop_returns_expected_loop(self):
        # Arrange
        # Act
        loop = self.engine.get_event_loop()

        # Assert
        assert loop == self.loop

    def test_message_qsize_at_max_blocks_on_put_command(self):
        # Arrange
        self.engine = LiveExecutionEngine(
            loop=self.loop,
            database=self.database,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
            config={"qsize": 1},
        )

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            order.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.engine.execute(submit_order)
        self.engine.execute(submit_order)

        # Assert
        assert self.engine.qsize() == 1
        assert self.engine.command_count == 0

    def test_message_qsize_at_max_blocks_on_put_event(self):
        # Arrange
        self.engine = LiveExecutionEngine(
            loop=self.loop,
            database=self.database,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
            config={"qsize": 1},
        )

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            order.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        event = TestStubs.event_order_submitted(order)

        # Act
        self.engine.execute(submit_order)
        self.engine.process(event)  # Add over max size

        # Assert
        assert self.engine.qsize() == 1
        assert self.engine.command_count == 0

    def test_start(self):
        async def run_test():
            # Arrange
            # Act
            self.engine.start()
            await asyncio.sleep(0.1)

            # Assert
            assert self.engine.state == ComponentState.RUNNING

            # Tear Down
            self.engine.stop()

        self.loop.run_until_complete(run_test())

    def test_kill_when_running_and_no_messages_on_queues(self):
        async def run_test():
            # Arrange
            # Act
            self.engine.start()
            await asyncio.sleep(0)
            self.engine.kill()

            # Assert
            assert self.engine.state == ComponentState.STOPPED

        self.loop.run_until_complete(run_test())

    def test_kill_when_not_running_with_messages_on_queue(self):
        async def run_test():
            # Arrange
            # Act
            self.engine.kill()

            # Assert
            assert self.engine.qsize() == 0

        self.loop.run_until_complete(run_test())

    def test_execute_command_places_command_on_queue(self):
        async def run_test():
            # Arrange
            self.engine.start()

            strategy = TradingStrategy(order_id_tag="001")
            strategy.register_trader(
                TraderId("TESTER", "000"),
                self.clock,
                self.logger,
            )

            self.engine.register_strategy(strategy)

            order = strategy.order_factory.market(
                AUDUSD_SIM.id,
                OrderSide.BUY,
                Quantity(100000),
            )

            submit_order = SubmitOrder(
                order.instrument_id,
                self.trader_id,
                self.account_id,
                strategy.id,
                PositionId.null(),
                order,
                self.uuid_factory.generate(),
                self.clock.timestamp_ns(),
            )

            # Act
            self.engine.execute(submit_order)
            await asyncio.sleep(0.1)

            # Assert
            assert self.engine.qsize() == 0
            assert self.engine.command_count == 1

            # Tear Down
            self.engine.stop()

        self.loop.run_until_complete(run_test())

    def test_handle_position_opening_with_position_id_none(self):
        async def run_test():
            # Arrange
            self.engine.start()

            strategy = TradingStrategy(order_id_tag="001")
            strategy.register_trader(
                TraderId("TESTER", "000"),
                self.clock,
                self.logger,
            )

            self.engine.register_strategy(strategy)

            order = strategy.order_factory.market(
                AUDUSD_SIM.id,
                OrderSide.BUY,
                Quantity(100000),
            )

            event = TestStubs.event_order_submitted(order)

            # Act
            self.engine.process(event)
            await asyncio.sleep(0.1)

            # Assert
            assert self.engine.qsize() == 0
            assert self.engine.event_count == 1

            # Tear Down
            self.engine.stop()

        self.loop.run_until_complete(run_test())

    def test_reconcile_state_with_no_active_orders(self):
        async def run_test():
            # Arrange
            self.engine.start()

            strategy = TradingStrategy(order_id_tag="001")
            strategy.register_trader(
                TraderId("TESTER", "000"),
                self.clock,
                self.logger,
            )

            self.engine.register_strategy(strategy)

            # Act
            await self.engine.reconcile_state()
            self.engine.stop()

            # Assert
            assert True  # Does not throw exception - logs: State reconciled.

        self.loop.run_until_complete(run_test())

    def test_reconcile_state_when_report_agrees_reconciles(self):
        async def run_test():
            # Arrange
            self.engine.start()

            strategy = TradingStrategy(order_id_tag="001")
            strategy.register_trader(
                TraderId("TESTER", "000"),
                self.clock,
                self.logger,
            )

            self.engine.register_strategy(strategy)

            order = strategy.order_factory.limit(
                AUDUSD_SIM.id,
                OrderSide.BUY,
                Quantity(100000),
                Price("1.00000"),
            )

            submit_order = SubmitOrder(
                AUDUSD_SIM.id,
                self.trader_id,
                self.account_id,
                strategy.id,
                PositionId.null(),
                order,
                self.uuid_factory.generate(),
                self.clock.timestamp_ns(),
            )

            self.engine.execute(submit_order)
            self.engine.process(TestStubs.event_order_submitted(order))
            self.engine.process(TestStubs.event_order_accepted(order))

            report = OrderStatusReport(
                cl_ord_id=order.cl_ord_id,
                order_id=OrderId("1"),  # <-- from stub event
                order_state=OrderState.ACCEPTED,
                filled_qty=Quantity(0),
                timestamp_ns=0,
            )

            self.client.add_order_status_report(report)

            await asyncio.sleep(0.1)  # Allow processing time

            # Act
            result = await self.engine.reconcile_state()
            self.engine.stop()

            # Assert
            assert result

        self.loop.run_until_complete(run_test())

    def test_reconcile_state_when_cancelled_reconciles(self):
        async def run_test():
            # Arrange
            self.engine.start()

            strategy = TradingStrategy(order_id_tag="001")
            strategy.register_trader(
                TraderId("TESTER", "000"),
                self.clock,
                self.logger,
            )

            self.engine.register_strategy(strategy)

            order = strategy.order_factory.limit(
                AUDUSD_SIM.id,
                OrderSide.BUY,
                Quantity(100000),
                Price("1.00000"),
            )

            submit_order = SubmitOrder(
                AUDUSD_SIM.id,
                self.trader_id,
                self.account_id,
                strategy.id,
                PositionId.null(),
                order,
                self.uuid_factory.generate(),
                self.clock.timestamp_ns(),
            )

            self.engine.execute(submit_order)
            self.engine.process(TestStubs.event_order_submitted(order))
            self.engine.process(TestStubs.event_order_accepted(order))

            report = OrderStatusReport(
                cl_ord_id=order.cl_ord_id,
                order_id=OrderId("1"),  # <-- from stub event
                order_state=OrderState.CANCELLED,
                filled_qty=Quantity(0),
                timestamp_ns=0,
            )

            self.client.add_order_status_report(report)

            await asyncio.sleep(0.1)  # Allow processing time

            # Act
            result = await self.engine.reconcile_state()
            self.engine.stop()

            # Assert
            assert result

        self.loop.run_until_complete(run_test())

    def test_reconcile_state_when_expired_reconciles(self):
        async def run_test():
            # Arrange
            self.engine.start()

            strategy = TradingStrategy(order_id_tag="001")
            strategy.register_trader(
                TraderId("TESTER", "000"),
                self.clock,
                self.logger,
            )

            self.engine.register_strategy(strategy)

            order = strategy.order_factory.limit(
                AUDUSD_SIM.id,
                OrderSide.BUY,
                Quantity(100000),
                Price("1.00000"),
            )

            submit_order = SubmitOrder(
                AUDUSD_SIM.id,
                self.trader_id,
                self.account_id,
                strategy.id,
                PositionId.null(),
                order,
                self.uuid_factory.generate(),
                self.clock.timestamp_ns(),
            )

            self.engine.execute(submit_order)
            self.engine.process(TestStubs.event_order_submitted(order))
            self.engine.process(TestStubs.event_order_accepted(order))

            report = OrderStatusReport(
                cl_ord_id=order.cl_ord_id,
                order_id=OrderId("1"),  # <-- from stub event
                order_state=OrderState.EXPIRED,
                filled_qty=Quantity(0),
                timestamp_ns=0,
            )

            self.client.add_order_status_report(report)

            await asyncio.sleep(0.01)

            # Act
            result = await self.engine.reconcile_state()
            self.engine.stop()

            # Assert
            assert result

        self.loop.run_until_complete(run_test())

    def test_reconcile_state_when_partially_filled_reconciles(self):
        async def run_test():
            # Arrange
            self.engine.start()

            strategy = TradingStrategy(order_id_tag="001")
            strategy.register_trader(
                TraderId("TESTER", "000"),
                self.clock,
                self.logger,
            )

            self.engine.register_strategy(strategy)

            order = strategy.order_factory.limit(
                AUDUSD_SIM.id,
                OrderSide.BUY,
                Quantity(100000),
                Price("1.00000"),
            )

            submit_order = SubmitOrder(
                AUDUSD_SIM.id,
                self.trader_id,
                self.account_id,
                strategy.id,
                PositionId.null(),
                order,
                self.uuid_factory.generate(),
                self.clock.timestamp_ns(),
            )

            self.engine.execute(submit_order)
            self.engine.process(TestStubs.event_order_submitted(order))
            self.engine.process(TestStubs.event_order_accepted(order))

            report = OrderStatusReport(
                cl_ord_id=order.cl_ord_id,
                order_id=OrderId("1"),  # <-- from stub event
                order_state=OrderState.PARTIALLY_FILLED,
                filled_qty=Quantity(70000),
                timestamp_ns=0,
            )

            trade1 = ExecutionReport(
                execution_id=ExecutionId("1"),
                cl_ord_id=order.cl_ord_id,
                order_id=OrderId("1"),
                last_qty=Decimal(50000),
                last_px=Decimal("1.00000"),
                commission_amount=Decimal("5.0"),
                commission_currency="USD",
                liquidity_side=LiquiditySide.MAKER,
                execution_ns=0,
                timestamp_ns=0,
            )

            trade2 = ExecutionReport(
                execution_id=ExecutionId("2"),
                cl_ord_id=order.cl_ord_id,
                order_id=OrderId("1"),
                last_qty=Decimal(20000),
                last_px=Decimal("1.00000"),
                commission_amount=Decimal("2.0"),
                commission_currency="USD",
                liquidity_side=LiquiditySide.MAKER,
                execution_ns=0,
                timestamp_ns=0,
            )

            self.client.add_order_status_report(report)
            self.client.add_trades_list(OrderId("1"), [trade1, trade2])

            await asyncio.sleep(0.01)

            # Act
            result = await self.engine.reconcile_state()
            self.engine.stop()

            # Assert
            assert result

        self.loop.run_until_complete(run_test())

    def test_reconcile_state_when_filled_reconciles(self):
        async def run_test():
            # Arrange
            self.engine.start()

            strategy = TradingStrategy(order_id_tag="001")
            strategy.register_trader(
                TraderId("TESTER", "000"),
                self.clock,
                self.logger,
            )

            self.engine.register_strategy(strategy)

            order = strategy.order_factory.limit(
                AUDUSD_SIM.id,
                OrderSide.BUY,
                Quantity(100000),
                Price("1.00000"),
            )

            submit_order = SubmitOrder(
                AUDUSD_SIM.id,
                self.trader_id,
                self.account_id,
                strategy.id,
                PositionId.null(),
                order,
                self.uuid_factory.generate(),
                self.clock.timestamp_ns(),
            )

            self.engine.execute(submit_order)
            self.engine.process(TestStubs.event_order_submitted(order))
            self.engine.process(TestStubs.event_order_accepted(order))

            report = OrderStatusReport(
                cl_ord_id=order.cl_ord_id,
                order_id=OrderId("1"),  # <-- from stub event
                order_state=OrderState.FILLED,
                filled_qty=Quantity(100000),
                timestamp_ns=0,
            )

            trade1 = ExecutionReport(
                execution_id=ExecutionId("1"),
                cl_ord_id=order.cl_ord_id,
                order_id=OrderId("1"),
                last_qty=Decimal(50000),
                last_px=Decimal("1.00000"),
                commission_amount=Decimal("5.0"),
                commission_currency="USD",
                liquidity_side=LiquiditySide.MAKER,
                execution_ns=0,
                timestamp_ns=0,
            )

            trade2 = ExecutionReport(
                execution_id=ExecutionId("2"),
                cl_ord_id=order.cl_ord_id,
                order_id=OrderId("1"),
                last_qty=Decimal(50000),
                last_px=Decimal("1.00000"),
                commission_amount=Decimal("2.0"),
                commission_currency="USD",
                liquidity_side=LiquiditySide.MAKER,
                execution_ns=0,
                timestamp_ns=0,
            )

            self.client.add_order_status_report(report)
            self.client.add_trades_list(OrderId("1"), [trade1, trade2])

            await asyncio.sleep(0.01)

            # Act
            result = await self.engine.reconcile_state()
            self.engine.stop()

            # Assert
            assert result

        self.loop.run_until_complete(run_test())
