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
import cProfile
import pstats
import time

import pytest

from nautilus_trader.analysis.performance import PerformanceAnalyzer
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.data.cache import DataCache
from nautilus_trader.execution.database import InMemoryExecutionDatabase
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.risk_engine import LiveRiskEngine
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Quantity
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
        self.trader_id = TraderId("TESTER-000")
        self.logger = Logger(self.clock, bypass_logging=True)

        self.account_id = AccountId(BINANCE.value, "001")

        self.portfolio = Portfolio(
            clock=self.clock,
            logger=self.logger,
        )

        self.analyzer = PerformanceAnalyzer()

        # Fresh isolated loop testing pattern
        self.loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self.loop)

        database = InMemoryExecutionDatabase(
            trader_id=self.trader_id, logger=self.logger
        )
        self.exec_engine = LiveExecutionEngine(
            loop=self.loop,
            database=database,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
        )

        self.risk_engine = LiveRiskEngine(
            loop=self.loop,
            exec_engine=self.exec_engine,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
        )

        exec_client = MockExecutionClient(
            client_id=ClientId("BINANCE"),
            venue_type=VenueType.EXCHANGE,
            account_id=self.account_id,
            engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
        )

        # Wire up components
        self.exec_engine.register_risk_engine(self.risk_engine)
        self.exec_engine.register_client(exec_client)
        self.exec_engine.process(TestStubs.event_account_state(self.account_id))
        self.portfolio.register_data_cache(DataCache(self.logger))
        self.portfolio.register_exec_cache(self.exec_engine.cache)

        self.strategy = TradingStrategy(order_id_tag="001")
        self.strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(self.strategy)

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

        self.benchmark.pedantic(execute_command, iterations=10_000, rounds=1)
        # ~0.0ms / ~0.2μs / 218ns minimum of 10,000 runs @ 1 iteration each run.

    def test_submit_order(self):
        self.exec_engine.start()
        time.sleep(0.1)

        async def run_test():
            def submit_order():
                order = self.strategy.order_factory.market(
                    BTCUSDT_BINANCE.id,
                    OrderSide.BUY,
                    Quantity.from_str("1.00000000"),
                )

                self.strategy.submit_order(order)

            self.benchmark.pedantic(submit_order, iterations=10_000, rounds=1)

        self.loop.run_until_complete(run_test())
        # ~0.0ms / ~25.3μs / 25326ns minimum of 10,000 runs @ 1 iteration each run.

    def test_submit_order_end_to_end(self):
        self.exec_engine.start()
        time.sleep(0.1)

        async def run_test():
            for _ in range(10000):
                order = self.strategy.order_factory.market(
                    BTCUSDT_BINANCE.id,
                    OrderSide.BUY,
                    Quantity.from_str("1.00000000"),
                )

                self.strategy.submit_order(order)

        stats_file = "perf_live_execution.prof"
        cProfile.runctx(
            "self.loop.run_until_complete(run_test())", globals(), locals(), stats_file
        )
        s = pstats.Stats(stats_file)
        s.strip_dirs().sort_stats("time").print_stats()
