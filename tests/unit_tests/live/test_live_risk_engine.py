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
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.logging import Logger
from nautilus_trader.config import LiveRiskEngineConfig
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.risk_engine import LiveRiskEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Quantity
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.mocks.exec_clients import MockExecutionClient
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


SIM = Venue("SIM")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")


class TestLiveRiskEngine:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.logger = Logger(self.clock, bypass=True)

        self.trader_id = TestIdStubs.trader_id()
        self.account_id = TestIdStubs.account_id()

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
            client_id=ClientId("SIM"),
            venue=SIM,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Wire up components
        self.exec_engine.register_client(self.exec_client)

    @pytest.mark.asyncio()
    async def test_start_when_loop_not_running_logs(self):
        # Arrange, Act
        self.risk_engine.start()

        # Assert
        assert True  # No exceptions raised
        self.risk_engine.stop()

    @pytest.mark.asyncio()
    async def test_message_qsize_at_max_blocks_on_put_command(self):
        # Arrange
        self.msgbus.deregister("RiskEngine.execute", self.risk_engine.execute)
        self.msgbus.deregister("RiskEngine.process", self.risk_engine.process)

        self.risk_engine = LiveRiskEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=LiveRiskEngineConfig(qsize=1),
        )

        strategy = Strategy()
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
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)
        self.risk_engine.execute(submit_order)
        await asyncio.sleep(0.1)

        # Assert
        assert self.risk_engine.cmd_qsize() == 1
        assert self.risk_engine.command_count == 0

    @pytest.mark.asyncio()
    async def test_message_qsize_at_max_blocks_on_put_event(self):
        # Arrange
        self.msgbus.deregister("RiskEngine.execute", self.risk_engine.execute)
        self.msgbus.deregister("RiskEngine.process", self.risk_engine.process)

        self.risk_engine = LiveRiskEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=LiveRiskEngineConfig(qsize=1),
        )

        strategy = Strategy()
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
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        event = TestEventStubs.order_submitted(order)

        # Act
        self.risk_engine.execute(submit_order)
        self.risk_engine.process(event)  # Add over max size
        await asyncio.sleep(0.1)

        # Assert
        assert self.risk_engine.cmd_qsize() == 1
        assert self.risk_engine.event_count == 0

    @pytest.mark.asyncio()
    async def test_start(self):
        # Arrange, Act
        self.risk_engine.start()
        await asyncio.sleep(0.1)

        # Assert
        assert self.risk_engine.is_running

        # Tear Down
        self.risk_engine.stop()

    @pytest.mark.asyncio()
    async def test_kill_when_running_and_no_messages_on_queues(self):
        # Arrange, Act
        self.risk_engine.start()
        await asyncio.sleep(0)
        self.risk_engine.kill()

        # Assert
        assert self.risk_engine.is_stopped

    @pytest.mark.asyncio()
    async def test_kill_when_not_running_with_messages_on_queue(self):
        # Arrange, Act
        self.risk_engine.kill()

        # Assert
        assert self.risk_engine.cmd_qsize() == 0
        assert self.risk_engine.evt_qsize() == 0

    @pytest.mark.asyncio()
    async def test_execute_command_places_command_on_queue(self):
        # Arrange
        self.risk_engine.start()

        strategy = Strategy()
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
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)
        await asyncio.sleep(0.1)

        # Assert
        assert self.risk_engine.cmd_qsize() == 0
        assert self.risk_engine.command_count == 1

        # Tear Down
        self.risk_engine.stop()
        await self.risk_engine.get_cmd_queue_task()
        await self.risk_engine.get_evt_queue_task()

    @pytest.mark.asyncio()
    async def test_handle_position_opening_with_position_id_none(self):
        # Arrange
        self.risk_engine.start()

        strategy = Strategy()
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
            Quantity.from_int(100_000),
        )

        event = TestEventStubs.order_submitted(order)

        # Act
        self.risk_engine.process(event)
        await asyncio.sleep(0.1)

        # Assert
        assert self.risk_engine.cmd_qsize() == 0
        assert self.risk_engine.event_count == 1

        # Tear Down
        self.risk_engine.stop()
        await self.risk_engine.get_cmd_queue_task()
        await self.risk_engine.get_evt_queue_task()
