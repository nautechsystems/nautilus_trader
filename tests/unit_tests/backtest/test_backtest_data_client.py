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

from nautilus_trader.backtest.data_client import BacktestMarketDataClient
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.trading.portfolio import Portfolio
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestBacktestDataClient:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(self.clock)

        self.cache = TestStubs.cache()

        self.portfolio = Portfolio(
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine = DataEngine(
            portfolio=self.portfolio,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )
        self.data_engine.process(USDJPY_SIM)

        self.client = BacktestMarketDataClient(
            client_id=ClientId("SIM"),
            engine=self.data_engine,
            clock=TestClock(),
            logger=self.logger,
        )

    def test_connect(self):
        # Arrange
        # Act
        self.client.connect()

        # Assert
        assert self.client.is_connected

    def test_disconnect(self):
        # Arrange
        self.client.connect()

        # Act
        self.client.disconnect()

        # Assert
        assert not self.client.is_connected

    def test_reset(self):
        # Arrange
        # Act
        self.client.reset()

        # Assert
        assert True  # No exceptions raised

    def test_dispose(self):
        # Arrange
        # Act
        self.client.dispose()

        # Assert
        assert True  # No exceptions raised

    def test_subscribe_instrument(self):
        # Arrange
        # Act
        self.client.subscribe_instrument(USDJPY_SIM.id)
        self.client.connect()
        self.client.subscribe_instrument(USDJPY_SIM.id)

        # Assert
        assert True  # No exceptions raised

    def test_subscribe_quote_ticks(self):
        # Arrange
        # Act
        self.client.subscribe_quote_ticks(USDJPY_SIM.id)
        self.client.connect()
        self.client.subscribe_quote_ticks(USDJPY_SIM.id)

        # Assert
        assert True  # No exceptions raised

    def test_subscribe_trade_ticks(self):
        # Arrange
        # Act
        self.client.subscribe_trade_ticks(USDJPY_SIM.id)
        self.client.connect()
        self.client.subscribe_trade_ticks(USDJPY_SIM.id)

        # Assert
        assert True  # No exceptions raised

    def test_subscribe_bars(self):
        # Arrange
        # Act
        self.client.subscribe_bars(TestStubs.bartype_gbpusd_1sec_mid())
        self.client.connect()
        self.client.subscribe_bars(TestStubs.bartype_gbpusd_1sec_mid())

        # Assert
        assert True  # No exceptions raised

    def test_unsubscribe_instrument(self):
        # Arrange
        # Act
        self.client.unsubscribe_instrument(USDJPY_SIM.id)
        self.client.connect()
        self.client.unsubscribe_instrument(USDJPY_SIM.id)

        # Assert
        assert True  # No exceptions raised

    def test_unsubscribe_quote_ticks(self):
        # Arrange
        # Act
        self.client.unsubscribe_quote_ticks(USDJPY_SIM.id)
        self.client.connect()
        self.client.unsubscribe_quote_ticks(USDJPY_SIM.id)

        # Assert
        assert True  # No exceptions raised

    def test_unsubscribe_trade_ticks(self):
        # Arrange
        # Act
        self.client.unsubscribe_trade_ticks(USDJPY_SIM.id)
        self.client.connect()
        self.client.unsubscribe_trade_ticks(USDJPY_SIM.id)

        # Assert
        assert True  # No exceptions raised

    def test_unsubscribe_bars(self):
        # Arrange
        # Act
        self.client.unsubscribe_bars(TestStubs.bartype_usdjpy_1min_bid())
        self.client.connect()
        self.client.unsubscribe_bars(TestStubs.bartype_usdjpy_1min_bid())

        # Assert
        assert True  # No exceptions raised

    def test_request_instrument(self):
        # Arrange
        # Act
        self.client.request_instrument(USDJPY_SIM.id, uuid4())
        self.client.connect()
        self.client.request_instrument(USDJPY_SIM.id, uuid4())

        # Assert
        assert True  # No exceptions raised

    def test_request_instruments(self):
        # Arrange
        # Act
        self.client.request_instruments(uuid4())
        self.client.connect()
        self.client.request_instruments(uuid4())

        # Assert
        assert True  # No exceptions raised

    def test_request_quote_ticks(self):
        # Arrange
        # Act
        self.client.request_quote_ticks(USDJPY_SIM.id, None, None, 0, uuid4())
        self.client.connect()
        self.client.request_quote_ticks(USDJPY_SIM.id, None, None, 0, uuid4())

        # Assert
        assert True  # No exceptions raised

    def test_request_trade_ticks(self):
        # Arrange
        # Act
        self.client.request_trade_ticks(USDJPY_SIM.id, None, None, 0, uuid4())
        self.client.connect()
        self.client.request_trade_ticks(USDJPY_SIM.id, None, None, 0, uuid4())

        # Assert
        assert True  # No exceptions raised

    def test_request_bars(self):
        # Arrange
        # Act
        self.client.request_bars(TestStubs.bartype_usdjpy_1min_bid(), None, None, 0, uuid4())
        self.client.connect()
        self.client.request_bars(TestStubs.bartype_usdjpy_1min_bid(), None, None, 0, uuid4())

        # Assert
        assert True  # No exceptions raised
