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

import asyncio
import unittest

from nautilus_trader.adapters.binance.data import BinanceDataClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.live.data import LiveDataEngine
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.trading.portfolio import Portfolio


# Requirements:
#    - An internet connection


class BinanceDataClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.trader_id = TraderId("TESTER", "001")

        # Fresh isolated loop testing pattern
        self.loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self.loop)

        # Setup logging
        logger = LiveLogger(
            clock=self.clock,
            name=self.trader_id.value,
            level_console=LogLevel.INFO,
            level_file=LogLevel.DEBUG,
            level_store=LogLevel.WARNING,
        )

        self.logger = LiveLogger(self.clock)

        self.portfolio = Portfolio(
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine = LiveDataEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
        )

        self.client = BinanceDataClient(
            credentials={},
            engine=self.data_engine,
            clock=self.clock,
            logger=logger,
        )

    def test_connect(self):
        # Arrange
        # Act
        self.client.connect()

        # Assert
        self.assertTrue(self.client.is_connected())

    def test_disconnect(self):
        # Arrange
        self.client.connect()

        # Act
        self.client.disconnect()

        # Assert
        self.assertFalse(self.client.is_connected())

    def test_reset(self):
        # Arrange
        # Act
        self.client.reset()

        # Assert
        self.assertTrue(True)  # No exceptions raised

    def test_dispose(self):
        # Arrange
        # Act
        self.client.dispose()

        # Assert
        self.assertTrue(True)  # No exceptions raised

    # def test_subscribe_instrument(self):
    #     # Arrange
    #     # Act
    #     self.client.subscribe_instrument(USDJPY_FXCM.symbol)
    #     self.client.connect()
    #     self.client.subscribe_instrument(USDJPY_FXCM.symbol)
    #
    #     # Assert
    #     self.assertTrue(True)  # Add with further functionality
    #
    # def test_subscribe_quote_ticks(self):
    #     # Arrange
    #     # Act
    #     self.client.subscribe_quote_ticks(USDJPY_FXCM.symbol)
    #     self.client.connect()
    #     self.client.subscribe_quote_ticks(USDJPY_FXCM.symbol)
    #
    #     # Assert
    #     self.assertTrue(True)  # Add with further functionality
    #
    # def test_subscribe_trade_ticks(self):
    #     # Arrange
    #     # Act
    #     self.client.subscribe_trade_ticks(USDJPY_FXCM.symbol)
    #     self.client.connect()
    #     self.client.subscribe_trade_ticks(USDJPY_FXCM.symbol)
    #
    #     # Assert
    #     self.assertTrue(True)  # Add with further functionality
    #
    # def test_subscribe_bars(self):
    #     # Arrange
    #     # Act
    #     self.client.subscribe_bars(TestStubs.bartype_gbpusd_1sec_mid())
    #     self.client.connect()
    #     self.client.subscribe_bars(TestStubs.bartype_gbpusd_1sec_mid())
    #
    #     # Assert
    #     self.assertTrue(True)  # Add with further functionality
    #
    # def test_unsubscribe_instrument(self):
    #     # Arrange
    #     # Act
    #     self.client.unsubscribe_instrument(USDJPY_FXCM.symbol)
    #     self.client.connect()
    #     self.client.unsubscribe_instrument(USDJPY_FXCM.symbol)
    #
    #     # Assert
    #     self.assertTrue(True)  # Add with further functionality
    #
    # def test_unsubscribe_quote_ticks(self):
    #     # Arrange
    #     # Act
    #     self.client.unsubscribe_quote_ticks(USDJPY_FXCM.symbol)
    #     self.client.connect()
    #     self.client.unsubscribe_quote_ticks(USDJPY_FXCM.symbol)
    #
    #     # Assert
    #     self.assertTrue(True)  # Add with further functionality
    #
    # def test_unsubscribe_trade_ticks(self):
    #     # Arrange
    #     # Act
    #     self.client.unsubscribe_trade_ticks(USDJPY_FXCM.symbol)
    #     self.client.connect()
    #     self.client.unsubscribe_trade_ticks(USDJPY_FXCM.symbol)
    #
    #     # Assert
    #     self.assertTrue(True)  # Add with further functionality
    #
    # def test_unsubscribe_bars(self):
    #     # Arrange
    #     # Act
    #     self.client.unsubscribe_bars(TestStubs.bartype_usdjpy_1min_bid())
    #     self.client.connect()
    #     self.client.unsubscribe_bars(TestStubs.bartype_usdjpy_1min_bid())
    #
    #     # Assert
    #     self.assertTrue(True)  # Add with further functionality
    #
    # def test_request_instrument(self):
    #     # Arrange
    #     # Act
    #     self.client.request_instrument(USDJPY_FXCM.symbol, uuid4())
    #     self.client.connect()
    #     self.client.request_instrument(USDJPY_FXCM.symbol, uuid4())
    #
    #     # Assert
    #     self.assertTrue(True)  # Add with further functionality
    #
    # def test_request_instruments(self):
    #     # Arrange
    #     # Act
    #     self.client.request_instruments(uuid4())
    #     self.client.connect()
    #     self.client.request_instruments(uuid4())
    #
    #     # Assert
    #     self.assertTrue(True)  # Add with further functionality
    #
    # def test_request_quote_ticks(self):
    #     # Arrange
    #     # Act
    #     self.client.request_quote_ticks(USDJPY_FXCM.symbol, None, None, 0, uuid4())
    #     self.client.connect()
    #     self.client.request_quote_ticks(USDJPY_FXCM.symbol, None, None, 0, uuid4())
    #
    #     # Assert
    #     self.assertTrue(True)  # Add with further functionality
    #
    # def test_request_trade_ticks(self):
    #     # Arrange
    #     # Act
    #     self.client.request_trade_ticks(USDJPY_FXCM.symbol, None, None, 0, uuid4())
    #     self.client.connect()
    #     self.client.request_trade_ticks(USDJPY_FXCM.symbol, None, None, 0, uuid4())
    #
    #     # Assert
    #     self.assertTrue(True)  # Add with further functionality
    #
    # def test_request_bars(self):
    #     # Arrange
    #     # Act
    #     self.client.request_bars(TestStubs.bartype_usdjpy_1min_bid(), None, None, 0, uuid4())
    #     self.client.connect()
    #     self.client.request_bars(TestStubs.bartype_usdjpy_1min_bid(), None, None, 0, uuid4())
    #
    #     # Assert
    #     self.assertTrue(True)  # Add with further functionality
