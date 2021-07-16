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
from nautilus_trader.common.enums import ComponentState
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.core.type import DataType
from nautilus_trader.data.messages import DataRequest
from nautilus_trader.data.messages import DataResponse
from nautilus_trader.data.messages import Subscribe
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.model.data.base import Data
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.msgbus.message_bus import MessageBus
from nautilus_trader.trading.portfolio import Portfolio
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


BITMEX = Venue("BITMEX")
BINANCE = Venue("BINANCE")
XBTUSD_BITMEX = TestInstrumentProvider.xbtusd_bitmex()
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class TestLiveDataEngine:
    def setup(self):
        # Fixture Setup
        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(self.clock)

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

        self.engine = LiveDataEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

    def teardown(self):
        self.engine.dispose()

    def test_start_when_loop_not_running_logs(self):
        # Arrange
        # Act
        self.engine.start()

        # Assert
        assert True  # No exceptions raised
        self.engine.stop()

    def test_message_qsize_at_max_blocks_on_put_data_command(self):
        # Arrange
        self.engine = LiveDataEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config={"qsize": 1},
        )

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(QuoteTick),
            handler=[].append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.engine.execute(subscribe)
        self.engine.execute(subscribe)

        # Assert
        assert self.engine.message_qsize() == 1
        assert self.engine.command_count == 0

    def test_message_qsize_at_max_blocks_on_send_request(self):
        # Arrange
        self.engine = LiveDataEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config={"qsize": 1},
        )

        handler = []
        request = DataRequest(
            client_id=ClientId("RANDOM"),
            data_type=DataType(
                QuoteTick,
                metadata={
                    "instrument_id": InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
                    "from_datetime": None,
                    "to_datetime": None,
                    "limit": 1000,
                },
            ),
            callback=handler.append,
            request_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.engine.send(request)
        self.engine.send(request)

        # Assert
        assert self.engine.message_qsize() == 1
        assert self.engine.command_count == 0

    def test_message_qsize_at_max_blocks_on_receive_response(self):
        # Arrange
        self.engine = LiveDataEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config={"qsize": 1},
        )

        response = DataResponse(
            client_id=ClientId("BINANCE"),
            data_type=DataType(QuoteTick),
            data=[],
            correlation_id=self.uuid_factory.generate(),
            response_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.engine.receive(response)
        self.engine.receive(response)  # Add over max size

        # Assert
        assert self.engine.message_qsize() == 1
        assert self.engine.command_count == 0

    def test_data_qsize_at_max_blocks_on_put_data(self):
        # Arrange
        self.engine = LiveDataEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config={"qsize": 1},
        )

        data = Data(1_000_000_000, 1_000_000_000)

        # Act
        self.engine.process(data)
        self.engine.process(data)  # Add over max size

        # Assert
        assert self.engine.data_qsize() == 1
        assert self.engine.data_count == 0

    def test_get_event_loop_returns_expected_loop(self):
        # Arrange
        # Act
        loop = self.engine.get_event_loop()

        # Assert
        assert loop == self.loop

    @pytest.mark.asyncio
    async def test_start(self):
        # Arrange
        # Act
        self.engine.start()
        await asyncio.sleep(0.1)

        # Assert
        assert self.engine.state == ComponentState.RUNNING

        # Tear Down
        self.engine.stop()

    @pytest.mark.asyncio
    async def test_kill_when_running_and_no_messages_on_queues(self):
        # Arrange
        # Act
        self.engine.start()
        await asyncio.sleep(0)
        self.engine.kill()

        # Assert
        assert self.engine.state == ComponentState.STOPPED

    @pytest.mark.asyncio
    async def test_kill_when_not_running_with_messages_on_queue(self):
        # Arrange
        # Act
        self.engine.kill()

        # Assert
        assert self.engine.data_qsize() == 0

    @pytest.mark.asyncio
    async def test_execute_command_processes_message(self):
        # Arrange
        self.engine.start()

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(QuoteTick),
            handler=[].append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.engine.execute(subscribe)
        await asyncio.sleep(0.1)

        # Assert
        assert self.engine.message_qsize() == 0
        assert self.engine.command_count == 1

        # Tear Down
        self.engine.stop()

    @pytest.mark.asyncio
    async def test_send_request_processes_message(self):
        # Arrange
        self.engine.start()

        handler = []
        request = DataRequest(
            client_id=ClientId("RANDOM"),
            data_type=DataType(
                QuoteTick,
                metadata={
                    "instrument_id": InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
                    "from_datetime": None,
                    "to_datetime": None,
                    "limit": 1000,
                },
            ),
            callback=handler.append,
            request_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.engine.send(request)
        await asyncio.sleep(0.1)

        # Assert
        assert self.engine.message_qsize() == 0
        assert self.engine.request_count == 1

        # Tear Down
        self.engine.stop()

    @pytest.mark.asyncio
    async def test_receive_response_processes_message(self):
        # Arrange
        self.engine.start()

        response = DataResponse(
            client_id=ClientId("BINANCE"),
            data_type=DataType(QuoteTick),
            data=[],
            correlation_id=self.uuid_factory.generate(),
            response_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.engine.receive(response)
        await asyncio.sleep(0.1)

        # Assert
        assert self.engine.message_qsize() == 0
        assert self.engine.response_count == 1

        # Tear Down
        self.engine.stop()

    @pytest.mark.asyncio
    async def test_process_data_processes_data(self):
        # Arrange
        self.engine.start()

        # Act
        tick = TestStubs.trade_tick_5decimal()

        # Act
        self.engine.process(tick)
        await asyncio.sleep(0.1)

        # Assert
        assert self.engine.data_qsize() == 0
        assert self.engine.data_count == 1

        # Tear Down
        self.engine.stop()
