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

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.data import Data
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.client import DataClient
from nautilus_trader.data.client import MarketDataClient
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.data.book import OrderBookDeltas
from nautilus_trader.model.data.book import OrderBookSnapshot
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.filters import NewsEvent
from nautilus_trader.trading.filters import NewsImpact


SIM = Venue("SIM")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class TestDataClient:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
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

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.venue = Venue("SIM")

        self.client = DataClient(
            client_id=ClientId("TEST_PROVIDER"),
            venue=self.venue,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

    def test_subscribe_when_not_implemented_logs_error(self):
        # Arrange
        data_type = DataType(Data, {"Type": "MyData"})

        # Act
        self.client.subscribe(data_type)

        # Assert
        # TODO(cs): Determine better way of asserting this than parsing logs

    def test_unsubscribe_when_not_implemented_logs_error(self):
        # Arrange
        data_type = DataType(Data, {"Type": "MyData"})

        # Act
        self.client.subscribe(data_type)

        # Assert
        # TODO(cs): Determine better way of asserting this than parsing logs

    def test_request_when_not_implemented_logs_error(self):
        # Arrange
        data_type = DataType(Data, {"Type": "MyData"})

        # Act
        self.client.request(data_type, UUID4())

        # Assert
        # TODO(cs): Determine better way of asserting this than parsing logs

    def test_handle_data_sends_to_data_engine(self):
        # Arrange
        data_type = DataType(NewsEvent, {"Type": "NEWS_WIRE"})
        data = NewsEvent(
            impact=NewsImpact.HIGH,
            name="Unemployment Rate",
            currency=USD,
            ts_event=0,
            ts_init=0,
        )
        generic_data = GenericData(data_type, data)

        # Act
        self.client._handle_data_py(generic_data)

        # Assert
        assert self.data_engine.data_count == 1

    def test_handle_data_response_sends_to_data_engine(self):
        # Arrange
        data_type = DataType(NewsEvent, {"Type": "NEWS_WIRE"})
        data = NewsEvent(
            impact=NewsImpact.HIGH,
            name="Unemployment Rate",
            currency=USD,
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.client._handle_data_response_py(data_type, data, UUID4())

        # Assert
        assert self.data_engine.response_count == 1


class TestMarketDataClient:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
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

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.venue = Venue("SIM")

        self.client = MarketDataClient(
            client_id=ClientId(self.venue.value),
            venue=self.venue,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

    def test_handle_instrument_sends_to_data_engine(self):
        # Arrange, Act
        self.client._handle_data_py(AUDUSD_SIM)

        # Assert
        assert self.data_engine.data_count == 1

    def test_handle_order_book_snapshot_sends_to_data_engine(self):
        # Arrange
        snapshot = OrderBookSnapshot(
            instrument_id=ETHUSDT_BINANCE.id,
            bids=[[1000, 1]],
            asks=[[1001, 1]],
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.client._handle_data_py(snapshot)

        # Assert
        assert self.data_engine.data_count == 1

    def test_handle_order_book_operations_sends_to_data_engine(self):
        # Arrange
        deltas = OrderBookDeltas(
            instrument_id=ETHUSDT_BINANCE.id,
            deltas=[],
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.client._handle_data_py(deltas)

        # Assert
        assert self.data_engine.data_count == 1

    def test_handle_ticker_sends_to_data_engine(self):
        # Arrange
        tick = TestDataStubs.ticker()

        # Act
        self.client._handle_data_py(tick)

        # Assert
        assert self.data_engine.data_count == 1

    def test_handle_quote_tick_sends_to_data_engine(self):
        # Arrange
        tick = TestDataStubs.quote_tick_5decimal()

        # Act
        self.client._handle_data_py(tick)

        # Assert
        assert self.data_engine.data_count == 1

    def test_handle_trade_tick_sends_to_data_engine(self):
        # Arrange
        tick = TestDataStubs.trade_tick_5decimal()

        # Act
        self.client._handle_data_py(tick)

        # Assert
        assert self.data_engine.data_count == 1

    def test_handle_bar_sends_to_data_engine(self):
        # Arrange
        bar = TestDataStubs.bar_5decimal()

        # Act
        self.client._handle_data_py(bar)

        # Assert
        assert self.data_engine.data_count == 1

    def test_handle_quote_ticks_sends_to_data_engine(self):
        # Arrange, Act
        self.client._handle_quote_ticks_py(AUDUSD_SIM.id, [], UUID4())

        # Assert
        assert self.data_engine.response_count == 1

    def test_handle_trade_ticks_sends_to_data_engine(self):
        # Arrange, Act
        self.client._handle_trade_ticks_py(AUDUSD_SIM.id, [], UUID4())

        # Assert
        assert self.data_engine.response_count == 1

    def test_handle_bars_sends_to_data_engine(self):
        # Arrange, Act
        self.client._handle_bars_py(
            TestDataStubs.bartype_gbpusd_1sec_mid(),
            [],
            None,
            UUID4(),
        )

        # Assert
        assert self.data_engine.response_count == 1
