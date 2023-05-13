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
from nautilus_trader.config import LiveDataEngineConfig
from nautilus_trader.core.data import Data
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.messages import DataRequest
from nautilus_trader.data.messages import DataResponse
from nautilus_trader.data.messages import Subscribe
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


BITMEX = Venue("BITMEX")
BINANCE = Venue("BINANCE")
XBTUSD_BITMEX = TestInstrumentProvider.xbtusd_bitmex()
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class TestLiveDataEngine:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.logger = Logger(self.clock, bypass=True)

        self.trader_id = TestIdStubs.trader_id()

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

        self.engine = LiveDataEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

    def teardown(self):
        self.engine.dispose()

    @pytest.mark.asyncio()
    async def test_start_when_loop_not_running_logs(self):
        # Arrange, Act
        self.engine.start()

        # Assert
        assert True  # No exceptions raised
        self.engine.stop()

    @pytest.mark.asyncio()
    async def test_message_qsize_at_max_blocks_on_put_data_command(self):
        # Arrange
        self.msgbus.deregister(endpoint="DataEngine.execute", handler=self.engine.execute)
        self.msgbus.deregister(endpoint="DataEngine.process", handler=self.engine.process)
        self.msgbus.deregister(endpoint="DataEngine.request", handler=self.engine.request)
        self.msgbus.deregister(endpoint="DataEngine.response", handler=self.engine.response)

        self.engine = LiveDataEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=LiveDataEngineConfig(qsize=1),
        )

        subscribe = Subscribe(
            client_id=None,
            venue=BINANCE,
            data_type=DataType(QuoteTick),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.engine.execute(subscribe)
        self.engine.execute(subscribe)
        await asyncio.sleep(0.1)

        # Assert
        assert self.engine.cmd_qsize() == 1
        assert self.engine.command_count == 0

    @pytest.mark.asyncio()
    async def test_message_qsize_at_max_blocks_on_send_request(self):
        # Arrange
        self.msgbus.deregister(endpoint="DataEngine.execute", handler=self.engine.execute)
        self.msgbus.deregister(endpoint="DataEngine.process", handler=self.engine.process)
        self.msgbus.deregister(endpoint="DataEngine.request", handler=self.engine.request)
        self.msgbus.deregister(endpoint="DataEngine.response", handler=self.engine.response)

        self.engine = LiveDataEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=LiveDataEngineConfig(qsize=1),
        )

        handler = []
        request = DataRequest(
            client_id=ClientId("RANDOM"),
            venue=None,
            data_type=DataType(
                QuoteTick,
                metadata={
                    "instrument_id": InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
                    "start": None,
                    "end": None,
                    "limit": 1000,
                },
            ),
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.engine.request(request)
        self.engine.request(request)
        await asyncio.sleep(0.1)

        # Assert
        assert self.engine.req_qsize() == 1
        assert self.engine.command_count == 0

    @pytest.mark.asyncio()
    async def test_message_qsize_at_max_blocks_on_receive_response(self):
        # Arrange
        self.msgbus.deregister(endpoint="DataEngine.execute", handler=self.engine.execute)
        self.msgbus.deregister(endpoint="DataEngine.process", handler=self.engine.process)
        self.msgbus.deregister(endpoint="DataEngine.request", handler=self.engine.request)
        self.msgbus.deregister(endpoint="DataEngine.response", handler=self.engine.response)

        self.engine = LiveDataEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=LiveDataEngineConfig(qsize=1),
        )

        response = DataResponse(
            client_id=ClientId("BINANCE"),
            venue=BINANCE,
            data_type=DataType(QuoteTick),
            data=[],
            correlation_id=UUID4(),
            response_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.engine.response(response)
        self.engine.response(response)  # Add over max size
        await asyncio.sleep(0.1)

        # Assert
        assert self.engine.res_qsize() == 1
        assert self.engine.command_count == 0

    @pytest.mark.asyncio()
    async def test_data_qsize_at_max_blocks_on_put_data(self):
        # Arrange
        self.msgbus.deregister(endpoint="DataEngine.execute", handler=self.engine.execute)
        self.msgbus.deregister(endpoint="DataEngine.process", handler=self.engine.process)
        self.msgbus.deregister(endpoint="DataEngine.request", handler=self.engine.request)
        self.msgbus.deregister(endpoint="DataEngine.response", handler=self.engine.response)

        self.engine = LiveDataEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=LiveDataEngineConfig(qsize=1),
        )

        data = Data(1_000_000_000, 1_000_000_000)

        # Act
        self.engine.process(data)
        self.engine.process(data)  # Add over max size
        await asyncio.sleep(0.1)

        # Assert
        assert self.engine.data_qsize() == 1
        assert self.engine.data_count == 0

    @pytest.mark.asyncio()
    async def test_start(self):
        # Arrange, Act
        self.engine.start()
        await asyncio.sleep(0.1)

        # Assert
        assert self.engine.is_running

        # Tear Down
        self.engine.stop()

    @pytest.mark.asyncio()
    async def test_kill_when_running_and_no_messages_on_queues(self):
        # Arrange, Act
        self.engine.start()
        await asyncio.sleep(0)
        self.engine.kill()

        # Assert
        assert self.engine.is_stopped

    @pytest.mark.asyncio()
    async def test_kill_when_not_running_with_messages_on_queue(self):
        # Arrange, Act
        self.engine.kill()

        # Assert
        assert self.engine.data_qsize() == 0

    @pytest.mark.asyncio()
    async def test_execute_command_processes_message(self):
        # Arrange
        self.engine.start()

        subscribe = Subscribe(
            client_id=None,
            venue=BINANCE,
            data_type=DataType(QuoteTick),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.engine.execute(subscribe)
        await asyncio.sleep(0.1)

        # Assert
        assert self.engine.cmd_qsize() == 0
        assert self.engine.command_count == 1

        # Tear Down
        self.engine.stop()

    @pytest.mark.asyncio()
    async def test_send_request_processes_message(self):
        # Arrange
        self.engine.start()

        handler = []
        request = DataRequest(
            client_id=ClientId("RANDOM"),
            venue=None,
            data_type=DataType(
                QuoteTick,
                metadata={
                    "instrument_id": InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
                    "start": None,
                    "end": None,
                    "limit": 1000,
                },
            ),
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.engine.request(request)
        await asyncio.sleep(0.1)

        # Assert
        assert self.engine.req_qsize() == 0
        assert self.engine.request_count == 1

        # Tear Down
        self.engine.stop()

    @pytest.mark.asyncio()
    async def test_receive_response_processes_message(self):
        # Arrange
        self.engine.start()

        response = DataResponse(
            client_id=ClientId("BINANCE"),
            venue=BINANCE,
            data_type=DataType(QuoteTick),
            data=[],
            correlation_id=UUID4(),
            response_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.engine.response(response)
        await asyncio.sleep(0.1)

        # Assert
        assert self.engine.res_qsize() == 0
        assert self.engine.response_count == 1

        # Tear Down
        self.engine.stop()

    @pytest.mark.asyncio()
    async def test_process_data_processes_data(self):
        # Arrange
        self.engine.start()

        # Act
        tick = TestDataStubs.trade_tick_5decimal()

        # Act
        self.engine.process(tick)
        await asyncio.sleep(0.1)

        # Assert
        assert self.engine.data_qsize() == 0
        assert self.engine.data_count == 1

        # Tear Down
        self.engine.stop()
