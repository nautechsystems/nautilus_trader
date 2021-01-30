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
from nautilus_trader.common.logging import TestLogger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.data.messages import DataRequest
from nautilus_trader.data.messages import DataResponse
from nautilus_trader.data.messages import Subscribe
from nautilus_trader.live.data_client import LiveDataClient
from nautilus_trader.live.data_engine import LiveDataEngine
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
        self.logger = TestLogger(self.clock, level_console=LogLevel.DEBUG)

        self.portfolio = Portfolio(
            clock=self.clock,
            logger=self.logger,
        )

        # Fresh isolated loop testing pattern
        self.loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self.loop)

        self.data_engine = LiveDataEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
        )

    def tearDown(self):
        self.data_engine.dispose()
        self.loop.stop()
        self.loop.close()

    def test_start_when_loop_not_running_logs(self):
        # Arrange
        # Act
        self.data_engine.start()

        # Assert
        self.assertTrue(True)  # No exceptions raised
        self.data_engine.stop()

    def test_message_qsize_at_max_blocks_on_put_data_command(self):
        # Arrange
        self.data_engine = LiveDataEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
            config={"qsize": 1}
        )

        subscribe = Subscribe(
            venue=BINANCE,
            data_type=QuoteTick,
            metadata={},
            handler=[].append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        # Act
        self.data_engine.execute(subscribe)
        self.data_engine.execute(subscribe)

        # Assert
        self.assertEqual(1, self.data_engine.message_qsize())
        self.assertEqual(0, self.data_engine.command_count)

    def test_message_qsize_at_max_blocks_on_send_request(self):
        # Arrange
        self.data_engine = LiveDataEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
            config={"qsize": 1}
        )

        handler = []
        request = DataRequest(
            venue=Venue("RANDOM"),
            data_type=QuoteTick,
            metadata={
                "Symbol": Symbol("SOMETHING", Venue("RANDOM")),
                "FromDateTime": None,
                "ToDateTime": None,
                "Limit": 1000,
            },
            callback=handler.append,
            request_id=self.uuid_factory.generate(),
            request_timestamp=self.clock.utc_now(),
        )

        # Act
        self.data_engine.send(request)
        self.data_engine.send(request)

        # Assert
        self.assertEqual(1, self.data_engine.message_qsize())
        self.assertEqual(0, self.data_engine.command_count)

    def test_message_qsize_at_max_blocks_on_receive_response(self):
        # Arrange
        self.data_engine = LiveDataEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
            config={"qsize": 1}
        )

        response = DataResponse(
            venue=Venue("BINANCE"),
            data_type=QuoteTick,
            metadata={},
            data=[],
            correlation_id=self.uuid_factory.generate(),
            response_id=self.uuid_factory.generate(),
            response_timestamp=self.clock.utc_now(),
        )

        # Act
        self.data_engine.receive(response)
        self.data_engine.receive(response)  # Add over max size

        # Assert
        self.assertEqual(1, self.data_engine.message_qsize())
        self.assertEqual(0, self.data_engine.command_count)

    def test_data_qsize_at_max_blocks_on_put_data(self):
        # Arrange
        self.data_engine = LiveDataEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
            config={"qsize": 1}
        )

        # Act
        self.data_engine.process("some_data")
        self.data_engine.process("some_data")  # Add over max size

        # Assert
        self.assertEqual(1, self.data_engine.data_qsize())
        self.assertEqual(0, self.data_engine.data_count)

    def test_get_event_loop_returns_expected_loop(self):
        # Arrange
        # Act
        loop = self.data_engine.get_event_loop()

        # Assert
        self.assertEqual(self.loop, loop)

    def test_start(self):
        async def run_test():
            # Arrange
            # Act
            self.data_engine.start()
            await asyncio.sleep(0.1)

            # Assert
            self.assertEqual(ComponentState.RUNNING, self.data_engine.state)

            # Tear Down
            self.data_engine.stop()

        self.loop.run_until_complete(run_test())

    def test_kill_when_running_and_no_messages_on_queues(self):
        async def run_test():
            # Arrange
            # Act
            self.data_engine.start()
            await asyncio.sleep(0)
            self.data_engine.kill()

            # Assert
            self.assertEqual(ComponentState.STOPPED, self.data_engine.state)

        self.loop.run_until_complete(run_test())

    def test_kill_when_not_running_with_messages_on_queue(self):
        async def run_test():
            # Arrange
            # Act
            self.data_engine.kill()

            # Assert
            self.assertEqual(0, self.data_engine.data_qsize())

        self.loop.run_until_complete(run_test())

    def test_execute_command_processes_message(self):
        async def run_test():
            # Arrange
            self.data_engine.start()

            subscribe = Subscribe(
                venue=BINANCE,
                data_type=QuoteTick,
                metadata={},
                handler=[].append,
                command_id=self.uuid_factory.generate(),
                command_timestamp=self.clock.utc_now(),
            )

            # Act
            self.data_engine.execute(subscribe)
            await asyncio.sleep(0.1)

            # Assert
            self.assertEqual(0, self.data_engine.message_qsize())
            self.assertEqual(1, self.data_engine.command_count)

            # Tear Down
            self.data_engine.stop()

        self.loop.run_until_complete(run_test())

    def test_send_request_processes_message(self):
        async def run_test():
            # Arrange
            self.data_engine.start()

            handler = []
            request = DataRequest(
                venue=Venue("RANDOM"),
                data_type=QuoteTick,
                metadata={
                    "Symbol": Symbol("SOMETHING", Venue("RANDOM")),
                    "FromDateTime": None,
                    "ToDateTime": None,
                    "Limit": 1000,
                },
                callback=handler.append,
                request_id=self.uuid_factory.generate(),
                request_timestamp=self.clock.utc_now(),
            )

            # Act
            self.data_engine.send(request)
            await asyncio.sleep(0.1)

            # Assert
            self.assertEqual(0, self.data_engine.message_qsize())
            self.assertEqual(1, self.data_engine.request_count)

            # Tear Down
            self.data_engine.stop()

        self.loop.run_until_complete(run_test())

    def test_receive_response_processes_message(self):
        async def run_test():
            # Arrange
            self.data_engine.start()

            response = DataResponse(
                venue=Venue("BINANCE"),
                data_type=QuoteTick,
                metadata={},
                data=[],
                correlation_id=self.uuid_factory.generate(),
                response_id=self.uuid_factory.generate(),
                response_timestamp=self.clock.utc_now(),
            )

            # Act
            self.data_engine.receive(response)
            await asyncio.sleep(0.1)

            # Assert
            self.assertEqual(0, self.data_engine.message_qsize())
            self.assertEqual(1, self.data_engine.response_count)

            # Tear Down
            self.data_engine.stop()

        self.loop.run_until_complete(run_test())

    def test_process_data_processes_data(self):
        async def run_test():
            # Arrange
            self.data_engine.start()

            # Act
            tick = TestStubs.trade_tick_5decimal()

            # Act
            self.data_engine.process(tick)
            await asyncio.sleep(0.1)

            # Assert
            self.assertEqual(0, self.data_engine.data_qsize())
            self.assertEqual(1, self.data_engine.data_count)

            # Tear Down
            self.data_engine.stop()

        self.loop.run_until_complete(run_test())


class LiveDataClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.logger = TestLogger(self.clock, level_console=LogLevel.DEBUG)

        self.portfolio = Portfolio(
            clock=self.clock,
            logger=self.logger,
        )

        # Fresh isolated loop testing pattern
        self.loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self.loop)

        self.engine = LiveDataEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
        )

        self.client = LiveDataClient(
            venue=BINANCE,
            engine=self.engine,
            clock=self.clock,
            logger=self.logger,
        )

    def test_dummy_test(self):
        # Arrange
        # Act
        # Assert
        self.assertTrue(True)  # No exception raised
