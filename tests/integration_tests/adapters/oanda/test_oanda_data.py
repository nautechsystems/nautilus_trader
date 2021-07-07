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
import concurrent.futures
import json
from unittest.mock import MagicMock

from nautilus_trader.adapters.oanda.data import OandaDataClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.core.type import DataType
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.data.messages import DataRequest
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.model.bar import Bar
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.bar import BarType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.trading.portfolio import Portfolio
from tests import TESTS_PACKAGE_ROOT
from tests.test_kit.mocks import ObjectStorer
from tests.test_kit.stubs import TestStubs


TEST_PATH = TESTS_PACKAGE_ROOT + "/integration_tests/adapters/oanda/responses/"

OANDA = Venue("OANDA")
AUDUSD = InstrumentId(Symbol("AUD/USD"), OANDA)


class TestOandaDataClient:
    def setup(self):
        # Fixture Setup
        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.trader_id = TraderId("TESTER-001")

        # Fresh isolated loop testing pattern
        self.loop = asyncio.new_event_loop()
        self.loop.set_debug(True)
        self.executor = concurrent.futures.ThreadPoolExecutor()
        self.loop.set_default_executor(self.executor)
        asyncio.set_event_loop(self.loop)

        # Setup logging
        logger = LiveLogger(
            loop=self.loop,
            clock=self.clock,
            trader_id=self.trader_id,
            level_stdout=LogLevel.DEBUG,
        )

        self.logger = LiveLogger(
            loop=self.loop,
            clock=self.clock,
        )

        self.cache = TestStubs.cache()

        self.portfolio = Portfolio(
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine = LiveDataEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.mock_oanda = MagicMock()

        self.client = OandaDataClient(
            client=self.mock_oanda,
            account_id="001",
            engine=self.data_engine,
            clock=self.clock,
            logger=logger,
        )

        self.data_engine.register_client(self.client)

        with open(TEST_PATH + "instruments.json") as response:
            instruments = json.load(response)

        self.mock_oanda.request.return_value = instruments

    def teardown(self):
        self.executor.shutdown(wait=True)
        self.loop.stop()
        self.loop.close()

    def test_connect(self):
        async def run_test():
            # Arrange, Act
            self.data_engine.start()  # Also connects client
            await asyncio.sleep(1)

            # Assert
            assert self.client.is_connected

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_disconnect(self):
        async def run_test():
            # Arrange
            self.data_engine.start()  # Also connects client
            await asyncio.sleep(1)

            # Act
            self.client.disconnect()
            await asyncio.sleep(0.5)

            # Assert
            assert not self.client.is_connected

        self.loop.run_until_complete(run_test())

    def test_reset(self):
        # Arrange
        # Act
        self.client.reset()

        # Assert
        assert not self.client.is_connected

    def test_dispose(self):
        # Arrange
        # Act
        self.client.dispose()

        # Assert
        assert not self.client.is_connected

    def test_subscribe_instrument(self):
        # Arrange
        self.client.connect()

        # Act
        self.client.subscribe_instrument(AUDUSD)

        # Assert
        assert AUDUSD in self.client.subscribed_instruments

    def test_subscribe_quote_ticks(self):
        async def run_test():
            # Arrange
            self.mock_oanda.request.return_value = {"type": {"HEARTBEAT": "0"}}
            self.data_engine.start()

            # Act
            self.client.subscribe_quote_ticks(AUDUSD)
            await asyncio.sleep(0.3)

            # Assert
            assert AUDUSD in self.client.subscribed_quote_ticks

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_subscribe_bars(self):
        # Arrange
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.MID)
        bar_type = BarType(instrument_id=AUDUSD, bar_spec=bar_spec)

        # Act
        self.client.subscribe_bars(bar_type)

        # Assert
        assert True

    def test_unsubscribe_instrument(self):
        # Arrange
        self.client.connect()

        # Act
        self.client.unsubscribe_instrument(AUDUSD)

        # Assert
        assert True

    def test_unsubscribe_quote_ticks(self):
        async def run_test():
            # Arrange
            self.mock_oanda.request.return_value = {"type": {"HEARTBEAT": "0"}}
            self.data_engine.start()

            self.client.subscribe_quote_ticks(AUDUSD)
            await asyncio.sleep(0.3)

            # # Act
            self.client.unsubscribe_quote_ticks(AUDUSD)
            await asyncio.sleep(0.3)

            # Assert
            assert AUDUSD not in self.client.subscribed_quote_ticks

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_unsubscribe_bars(self):
        # Arrange
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.MID)
        bar_type = BarType(instrument_id=AUDUSD, bar_spec=bar_spec)

        # Act
        self.client.unsubscribe_bars(bar_type)

        # Assert
        assert True

    def test_request_instrument(self):
        async def run_test():
            # Arrange
            self.data_engine.start()  # Also starts client

            # Act
            self.client.request_instrument(AUDUSD, uuid4())
            await asyncio.sleep(1)

            # Assert
            # Instruments additionally requested on start
            assert self.data_engine.response_count == 1

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_request_instruments(self):
        async def run_test():
            # Arrange
            self.data_engine.start()  # Also starts client
            await asyncio.sleep(0.5)

            # Act
            self.client.request_instruments(uuid4())
            await asyncio.sleep(1)

            # Assert
            # Instruments additionally requested on start
            assert self.data_engine.response_count == 1

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_request_bars(self):
        async def run_test():
            # Arrange
            with open(TEST_PATH + "instruments.json") as response:
                instruments = json.load(response)

            # Arrange
            with open(TEST_PATH + "bars.json") as response:
                bars = json.load(response)

            self.mock_oanda.request.side_effect = [instruments, bars]

            handler = ObjectStorer()
            self.data_engine.start()
            await asyncio.sleep(0.3)

            bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.MID)
            bar_type = BarType(instrument_id=AUDUSD, bar_spec=bar_spec)

            request = DataRequest(
                client_id=ClientId(OANDA.value),
                data_type=DataType(
                    Bar,
                    metadata={
                        "bar_type": bar_type,
                        "from_datetime": None,
                        "to_datetime": None,
                        "limit": 1000,
                    },
                ),
                callback=handler.store,
                request_id=self.uuid_factory.generate(),
                timestamp_ns=self.clock.timestamp_ns(),
            )

            # Act
            self.data_engine.send(request)

            # Allow time for request to be sent, processed and response returned
            await asyncio.sleep(1)

            # Assert
            assert self.data_engine.response_count == 1
            assert handler.count == 1
            # Final bar incomplete so becomes partial
            assert len(handler.get_store()[0]) == 99

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()
            self.data_engine.dispose()

        self.loop.run_until_complete(run_test())
