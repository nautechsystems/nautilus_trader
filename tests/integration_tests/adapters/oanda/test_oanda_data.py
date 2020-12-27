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

from nautilus_trader.adapters.oanda.data import OandaDataClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.common.messages import DataRequest
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.live.data import LiveDataEngine
from nautilus_trader.model.bar import Bar
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.bar import BarType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.trading.portfolio import Portfolio
from tests.test_kit.mocks import ObjectStorer


OANDA = Venue("OANDA")
AUDUSD = Symbol("AUD/USD", OANDA)

# Requirements:
#    - An internet connection
#    - Environment variable OANDA_API_TOKEN with a valid practice account api token
#    - Environment variable OANDA_ACCOUNT_ID with a valid practice `accountID`


class OandaDataClientTests(unittest.TestCase):

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

        credentials = {
            "api_token": "OANDA_API_TOKEN",
            "account_id": "OANDA_ACCOUNT_ID",
        }

        self.client = OandaDataClient(
            credentials=credentials,
            engine=self.data_engine,
            clock=self.clock,
            logger=logger,
        )

        self.data_engine.register_client(self.client)

    def tearDown(self):
        self.loop.stop()
        self.loop.close()

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

    def test_subscribe_instrument(self):
        # Arrange
        self.client.connect()

        # Act
        self.client.subscribe_instrument(AUDUSD)

        # Assert
        self.assertTrue(True)

    def test_subscribe_quote_ticks(self):
        async def run_test():
            # Arrange
            self.data_engine.start()

            # Act
            self.client.subscribe_quote_ticks(AUDUSD)

            # Assert
            self.assertTrue(True)

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_task()

        self.loop.run_until_complete(run_test())

    def test_subscribe_bars(self):
        # Arrange
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.MID)
        bar_type = BarType(symbol=AUDUSD, bar_spec=bar_spec)

        # Act
        self.client.subscribe_bars(bar_type)

        # Assert
        self.assertTrue(True)

    def test_unsubscribe_instrument(self):
        # Arrange
        self.client.connect()

        # Act
        self.client.unsubscribe_instrument(AUDUSD)

        # Assert
        self.assertTrue(True)

    def test_unsubscribe_quote_ticks(self):
        # Arrange
        # Act
        self.client.unsubscribe_quote_ticks(AUDUSD)

        # Assert
        self.assertTrue(True)

    def test_unsubscribe_bars(self):
        # Arrange
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.MID)
        bar_type = BarType(symbol=AUDUSD, bar_spec=bar_spec)

        # Act
        self.client.unsubscribe_bars(bar_type)

        # Assert
        self.assertTrue(True)

    def test_request_instrument(self):
        async def run_test():
            # Arrange
            self.data_engine.start()

            # Act
            self.client.request_instrument(AUDUSD, uuid4())

            await asyncio.sleep(2)

            # Assert
            # Instruments additionally requested on start
            self.assertEqual(2, self.data_engine.response_count)

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_task()

        self.loop.run_until_complete(run_test())

    def test_request_instruments(self):
        async def run_test():
            # Arrange
            self.data_engine.start()

            # Act
            self.client.request_instruments(uuid4())

            await asyncio.sleep(2)

            # Assert
            # Instruments additionally requested on start
            self.assertEqual(2, self.data_engine.response_count)

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_task()

        self.loop.run_until_complete(run_test())

    def test_request_bars(self):
        async def run_test():
            # Arrange
            handler = ObjectStorer()
            self.data_engine.start()

            # Allow data engine to spool up and request instruments
            await asyncio.sleep(2)

            bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.MID)
            bar_type = BarType(symbol=AUDUSD, bar_spec=bar_spec)

            request = DataRequest(
                venue=OANDA,
                data_type=Bar,
                metadata={
                    "BarType": bar_type,
                    "FromDateTime": None,
                    "ToDateTime": None,
                    "Limit": 1000,
                },
                callback=handler.store_2,
                request_id=self.uuid_factory.generate(),
                request_timestamp=self.clock.utc_now(),
            )

            # Act
            self.data_engine.send(request)

            # Allow time for request to be sent, processed and response returned
            await asyncio.sleep(2)

            # Assert
            self.assertEqual(2, self.data_engine.response_count)
            self.assertEqual(1, handler.count)

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_task()
            self.data_engine.dispose()

        self.loop.run_until_complete(run_test())
