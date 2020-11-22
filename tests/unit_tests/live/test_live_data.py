# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

import time
import unittest

from nautilus_trader.backtest.loaders import InstrumentLoader
from nautilus_trader.backtest.logging import TestLogger
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.messages import Connect
from nautilus_trader.common.messages import DataRequest
from nautilus_trader.common.messages import DataResponse
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.live.data import LiveDataEngine
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.trading.portfolio import Portfolio
from tests.test_kit.stubs import TestStubs


BITMEX = Venue("BITMEX")
BINANCE = Venue("BINANCE")
XBTUSD_BITMEX = InstrumentLoader.xbtusd_bitmex()
BTCUSDT_BINANCE = InstrumentLoader.btcusdt_binance()
ETHUSDT_BINANCE = InstrumentLoader.ethusdt_binance()


class LiveDataEngineTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = TestLogger(self.clock)

        self.portfolio = Portfolio(
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

        self.data_engine = LiveDataEngine(
            portfolio=self.portfolio,
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

    def test_start(self):
        # Arrange
        # Act
        self.data_engine.start()

        # Assert
        # TODO: Implement test

    def test_given_execute_command_places_message_on_queue(self):
        # Arrange
        self.data_engine.start()

        connect = Connect(
            venue=BINANCE,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        # Act
        self.data_engine.execute(connect)

        time.sleep(0.1)

        # Assert
        self.assertEqual(1, self.data_engine.queue_size())

    def test_send_request_places_message_on_queue(self):
        # Arrange
        handler = []
        request = DataRequest(
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

        time.sleep(0.1)

        # Assert
        self.assertEqual(1, self.data_engine.queue_size())

    def test_receive_response_places_message_on_queue(self):
        # Arrange
        response = DataResponse(
            data_type=QuoteTick,
            metadata={},  # Malformed response anyway
            data=[],
            correlation_id=self.uuid_factory.generate(),
            response_id=self.uuid_factory.generate(),
            response_timestamp=self.clock.utc_now(),
        )

        # Act
        self.data_engine.receive(response)

        time.sleep(0.1)

        # Assert
        self.assertEqual(1, self.data_engine.queue_size())

    def test_process_data_places_data_on_queue(self):
        # Arrange
        # Act
        tick = TestStubs.trade_tick_5decimal()

        # Act
        self.data_engine.process(tick)

        # Assert
        self.assertEqual(1, self.data_engine.queue_size())
