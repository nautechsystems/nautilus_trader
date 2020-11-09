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

import unittest

from nautilus_trader.backtest.loaders import InstrumentLoader
from nautilus_trader.backtest.logging import TestLogger
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.messages import Connect
from nautilus_trader.common.messages import DataRequest
from nautilus_trader.common.messages import DataResponse
from nautilus_trader.common.messages import Disconnect
from nautilus_trader.common.messages import KillSwitch
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.trading.portfolio import Portfolio
from tests.test_kit.stubs import TestStubs


FXCM = Venue("FXCM")
BINANCE = Venue("BINANCE")
AUDUSD_FXCM = InstrumentLoader.default_fx_ccy(TestStubs.symbol_audusd_fxcm())
USDJPY_FXCM = InstrumentLoader.default_fx_ccy(TestStubs.symbol_usdjpy_fxcm())


class DataEngineTests(unittest.TestCase):

    def setUp(self):
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = TestLogger(self.clock)

        self.portfolio = Portfolio(
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

        self.data_engine = DataEngine(
            portfolio=self.portfolio,
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

        self.portfolio.register_cache(self.data_engine.cache)

    def test_registered_venues_when_nothing_registered_returns_empty_list(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual([], self.data_engine.registered_venues)

    def test_subscribed_instruments_when_nothing_subscribed_returns_empty_list(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual([], self.data_engine.subscribed_instruments)

    def test_subscribed_quote_ticks_when_nothing_subscribed_returns_empty_list(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual([], self.data_engine.subscribed_quote_ticks)

    def test_subscribed_trade_ticks_when_nothing_subscribed_returns_empty_list(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual([], self.data_engine.subscribed_trade_ticks)

    def test_subscribed_bars_when_nothing_subscribed_returns_empty_list(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual([], self.data_engine.subscribed_bars)

    def test_reset(self):
        # Arrange
        # Act
        self.data_engine.reset()

        # Assert
        self.assertEqual(0, self.data_engine.command_count)
        self.assertEqual(0, self.data_engine.data_count)
        self.assertEqual(0, self.data_engine.request_count)
        self.assertEqual(0, self.data_engine.response_count)

    def test_dispose(self):
        # Arrange
        # Act
        self.data_engine.dispose()

        # Assert
        self.assertEqual(0, self.data_engine.command_count)
        self.assertEqual(0, self.data_engine.data_count)
        self.assertEqual(0, self.data_engine.request_count)
        self.assertEqual(0, self.data_engine.response_count)

    def test_given_kill_switch_currently_does_nothing(self):
        # Arrange
        kill = KillSwitch(
            trader_id=TraderId("TESTER", "000"),
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        # Act
        self.data_engine.execute(kill)

        # Assert
        self.assertEqual(1, self.data_engine.command_count)

    def test_given_connect_when_no_data_clients_registered_does_nothing(self):
        # Arrange
        connect = Connect(
            venue=BINANCE,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        # Act
        self.data_engine.execute(connect)

        # Assert
        self.assertEqual(1, self.data_engine.command_count)

    def test_given_disconnect_when_no_data_clients_registered_does_nothing(self):
        # Arrange
        disconnect = Disconnect(
            venue=BINANCE,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        # Act
        self.data_engine.execute(disconnect)

        # Assert
        self.assertEqual(1, self.data_engine.command_count)

    def test_given_request_when_no_data_clients_registered_does_nothing(self):
        # Arrange
        handler = []
        request = DataRequest(
            data_type=QuoteTick,
            metadata={
                "Symbol": AUDUSD_FXCM.symbol,
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

        # Assert
        self.assertEqual(1, self.data_engine.request_count)

    def test_given_response_when_no_data_clients_registered_does_nothing(self):
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

        # Assert
        self.assertEqual(1, self.data_engine.response_count)
