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
import unittest

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.enums import ComponentState
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.data.messages import DataRequest
from nautilus_trader.data.messages import DataResponse
from nautilus_trader.data.messages import Subscribe
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.model.data import Data
from nautilus_trader.model.data import DataType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.trading.portfolio import Portfolio
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


BITMEX = Venue("BITMEX")
BINANCE = Venue("BINANCE")
XBTUSD_BITMEX = TestInstrumentProvider.xbtusd_bitmex()
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class LiveDataEngineTests(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(self.clock, level_stdout=LogLevel.DEBUG)

        self.cache = TestStubs.cache()

        self.portfolio = Portfolio(
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Fresh isolated loop testing pattern
        self.loop = asyncio.new_event_loop()
        self.loop.set_debug(True)
        asyncio.set_event_loop(self.loop)

        self.engine = LiveDataEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

    def tearDown(self):
        self.engine.dispose()
        self.loop.stop()
        self.loop.close()

    def test_start_when_loop_not_running_logs(self):
        # Arrange
        # Act
        self.engine.start()

        # Assert
        self.assertTrue(True)  # No exceptions raised
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
        self.assertEqual(1, self.engine.message_qsize())
        self.assertEqual(0, self.engine.command_count)

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
        self.assertEqual(1, self.engine.message_qsize())
        self.assertEqual(0, self.engine.command_count)

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
        self.assertEqual(1, self.engine.message_qsize())
        self.assertEqual(0, self.engine.command_count)

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
        self.assertEqual(1, self.engine.data_qsize())
        self.assertEqual(0, self.engine.data_count)

    def test_get_event_loop_returns_expected_loop(self):
        # Arrange
        # Act
        loop = self.engine.get_event_loop()

        # Assert
        self.assertEqual(self.loop, loop)

    def test_start(self):
        async def run_test():
            # Arrange
            # Act
            self.engine.start()
            await asyncio.sleep(0.1)

            # Assert
            self.assertEqual(ComponentState.RUNNING, self.engine.state)

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
            self.assertEqual(ComponentState.STOPPED, self.engine.state)

        self.loop.run_until_complete(run_test())

    def test_kill_when_not_running_with_messages_on_queue(self):
        async def run_test():
            # Arrange
            # Act
            self.engine.kill()

            # Assert
            self.assertEqual(0, self.engine.data_qsize())

        self.loop.run_until_complete(run_test())

    def test_execute_command_processes_message(self):
        async def run_test():
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
            self.assertEqual(0, self.engine.message_qsize())
            self.assertEqual(1, self.engine.command_count)

            # Tear Down
            self.engine.stop()

        self.loop.run_until_complete(run_test())

    def test_send_request_processes_message(self):
        async def run_test():
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
            self.assertEqual(0, self.engine.message_qsize())
            self.assertEqual(1, self.engine.request_count)

            # Tear Down
            self.engine.stop()

        self.loop.run_until_complete(run_test())

    def test_receive_response_processes_message(self):
        async def run_test():
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
            self.assertEqual(0, self.engine.message_qsize())
            self.assertEqual(1, self.engine.response_count)

            # Tear Down
            self.engine.stop()

        self.loop.run_until_complete(run_test())

    def test_process_data_processes_data(self):
        async def run_test():
            # Arrange
            self.engine.start()

            # Act
            tick = TestStubs.trade_tick_5decimal()

            # Act
            self.engine.process(tick)
            await asyncio.sleep(0.1)

            # Assert
            self.assertEqual(0, self.engine.data_qsize())
            self.assertEqual(1, self.engine.data_count)

            # Tear Down
            self.engine.stop()

        self.loop.run_until_complete(run_test())
