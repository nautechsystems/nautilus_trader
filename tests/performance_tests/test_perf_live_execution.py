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

import pytest

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.risk_engine import LiveRiskEngine
from nautilus_trader.model.commands.trading import SubmitOrder
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Quantity
from nautilus_trader.msgbus.message_bus import MessageBus
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.mocks import MockExecutionClient
from tests.test_kit.performance import PerformanceHarness
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


BINANCE = Venue("BINANCE")
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()


class TestLiveExecutionPerformance(PerformanceHarness):
    def setup(self):
        # Fixture Setup
        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.trader_id = TestStubs.trader_id()
        self.logger = Logger(self.clock, bypass=True)

        self.account_id = AccountId(BINANCE.value, "001")

        self.msgbus = MessageBus(
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

        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.data_engine = LiveDataEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_engine = LiveExecutionEngine(
            loop=self.loop,
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.risk_engine = LiveRiskEngine(
            loop=self.loop,
            exec_engine=self.exec_engine,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        exec_client = MockExecutionClient(
            client_id=ClientId("BINANCE"),
            venue_type=VenueType.EXCHANGE,
            account_id=self.account_id,
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency account
            engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
        )

        # Wire up components
        self.exec_engine.register_client(exec_client)
        self.exec_engine.process(TestStubs.event_account_state(self.account_id))

        self.strategy = TradingStrategy(order_id_tag="001")
        self.strategy.register(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            portfolio=self.portfolio,
            data_engine=self.data_engine,
            risk_engine=self.risk_engine,
            clock=self.clock,
            logger=self.logger,
        )

    @pytest.fixture(autouse=True)
    @pytest.mark.benchmark(disable_gc=True, warmup=True)
    def setup_benchmark(self, benchmark):
        self.benchmark = benchmark

    def submit_order(self):
        order = self.strategy.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.00000000"),
        )

        self.strategy.submit_order(order)

    def test_execute_command(self):
        order = self.strategy.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.00000000"),
        )

        command = SubmitOrder(
            self.trader_id,
            self.strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        def execute_command():
            self.exec_engine.execute(command)

        self.benchmark.pedantic(execute_command, iterations=100, rounds=100, warmup_rounds=5)
        # ~0.0ms / ~0.2μs / 218ns minimum of 10,000 runs @ 1 iteration each run.

    @pytest.mark.asyncio
    async def test_submit_order(self):
        self.exec_engine.start()
        await asyncio.sleep(1)

        def submit_order():
            order = self.strategy.order_factory.market(
                BTCUSDT_BINANCE.id,
                OrderSide.BUY,
                Quantity.from_str("1.00000000"),
            )

            self.strategy.submit_order(order)

        self.benchmark.pedantic(submit_order, iterations=100, rounds=100, warmup_rounds=5)
        # ~0.0ms / ~25.3μs / 25326ns minimum of 10,000 runs @ 1 iteration each run.

    @pytest.mark.asyncio
    async def test_submit_order_end_to_end(self):
        self.exec_engine.start()
        await asyncio.sleep(1)

        def run():
            for _ in range(1000):
                order = self.strategy.order_factory.market(
                    BTCUSDT_BINANCE.id,
                    OrderSide.BUY,
                    Quantity.from_str("1.00000000"),
                )

                self.strategy.submit_order(order)

        self.benchmark.pedantic(run, rounds=10, warmup_rounds=5)
