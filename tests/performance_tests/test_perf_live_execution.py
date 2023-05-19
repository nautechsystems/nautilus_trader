# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.risk_engine import LiveRiskEngine
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Quantity
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.mocks.exec_clients import MockExecutionClient
from nautilus_trader.test_kit.performance import PerformanceHarness
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


BINANCE = Venue("BINANCE")
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()


class TestLiveExecutionPerformance(PerformanceHarness):
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.logger = Logger(self.clock, bypass=True)

        self.trader_id = TestIdStubs.trader_id()
        self.account_id = AccountId(f"{BINANCE.value}-001")

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestComponentStubs.cache()

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

        self.exec_client = MockExecutionClient(
            client_id=ClientId("BINANCE"),
            venue=BINANCE,
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency account
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )
        self.portfolio.update_account(TestEventStubs.margin_account_state())
        self.exec_engine.register_client(self.exec_client)

        self.strategy = Strategy()
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
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

    @pytest.mark.asyncio()
    def test_execute_command(self):
        order = self.strategy.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.00000000"),
        )

        command = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        def execute_command():
            self.exec_engine.execute(command)

        self.benchmark.pedantic(execute_command, iterations=100, rounds=100, warmup_rounds=5)
        # ~0.0ms / ~0.2μs / 218ns minimum of 10,000 runs @ 1 iteration each run.

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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
