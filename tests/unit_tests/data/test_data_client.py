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

import pytest

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.data.client import DataClient
from nautilus_trader.data.client import MarketDataClient
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.enums import BookLevel
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from nautilus_trader.model.orderbook.data import OrderBookSnapshot
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.trading.filters import NewsEvent
from nautilus_trader.trading.filters import NewsImpact
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


SIM = Venue("SIM")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class TestDataClient:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(self.clock)

        self.trader_id = TestStubs.trader_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
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

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.venue = Venue("SIM")

        self.client = DataClient(
            client_id=ClientId("TEST_PROVIDER"),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

    def test_subscribe_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            self.client.subscribe(DataType(str))

    def test_unsubscribe_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            self.client.unsubscribe(DataType(str))

    def test_request_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            self.client.request(DataType(str), self.uuid_factory.generate())

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
        self.client._handle_data_response_py(data_type, data, self.uuid_factory.generate())

        # Assert
        assert self.data_engine.response_count == 1


class TestMarketDataClient:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(self.clock)

        self.trader_id = TestStubs.trader_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
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

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.venue = Venue("SIM")

        self.client = MarketDataClient(
            client_id=ClientId(self.venue.value),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

    def test_subscribe_instruments_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            self.client.subscribe_instruments()

    def test_subscribe_instrument_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            self.client.subscribe_instrument(AUDUSD_SIM.id)

    def test_subscribe_order_book_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            self.client.subscribe_order_book_snapshots(AUDUSD_SIM.id, 2, 0)

    def test_subscribe_ticker_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            self.client.subscribe_ticker(AUDUSD_SIM.id)

    def test_subscribe_quote_ticks_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            self.client.subscribe_quote_ticks(AUDUSD_SIM.id)

    def test_subscribe_trade_ticks_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            self.client.subscribe_trade_ticks(AUDUSD_SIM.id)

    def test_subscribe_bars_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            self.client.subscribe_bars(TestStubs.bartype_gbpusd_1sec_mid())

    def test_unsubscribe_instrument_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            self.client.unsubscribe_instrument(AUDUSD_SIM.id)

    def test_unsubscribe_order_book_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            self.client.unsubscribe_order_book_snapshots(AUDUSD_SIM.id)

    def test_unsubscribe_ticker_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            self.client.unsubscribe_ticker(AUDUSD_SIM.id)

    def test_unsubscribe_quote_ticks_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            self.client.unsubscribe_quote_ticks(AUDUSD_SIM.id)

    def test_unsubscribe_trade_ticks_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            self.client.unsubscribe_trade_ticks(AUDUSD_SIM.id)

    def test_unsubscribe_bars_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            self.client.unsubscribe_bars(TestStubs.bartype_gbpusd_1sec_mid())

    def test_request_quote_ticks_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            self.client.request_quote_ticks(None, None, None, 0, None)

    def test_request_trade_ticks_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            self.client.request_trade_ticks(None, None, None, 0, None)

    def test_request_bars_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            self.client.request_bars(None, None, None, 0, None)

    def test_unavailable_methods_when_none_given_returns_empty_list(self):
        # Arrange
        # Act
        result = self.client.unavailable_methods()

        # Assert
        assert result == []

    def test_handle_instrument_sends_to_data_engine(self):
        # Arrange
        # Act
        self.client._handle_data_py(AUDUSD_SIM)

        # Assert
        assert self.data_engine.data_count == 1

    def test_handle_order_book_snapshot_sends_to_data_engine(self):
        # Arrange
        snapshot = OrderBookSnapshot(
            instrument_id=ETHUSDT_BINANCE.id,
            level=BookLevel.L2,
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
            level=BookLevel.L2,
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
        tick = TestStubs.ticker()

        # Act
        self.client._handle_data_py(tick)

        # Assert
        assert self.data_engine.data_count == 1

    def test_handle_quote_tick_sends_to_data_engine(self):
        # Arrange
        tick = TestStubs.quote_tick_5decimal()

        # Act
        self.client._handle_data_py(tick)

        # Assert
        assert self.data_engine.data_count == 1

    def test_handle_trade_tick_sends_to_data_engine(self):
        # Arrange
        tick = TestStubs.trade_tick_5decimal()

        # Act
        self.client._handle_data_py(tick)

        # Assert
        assert self.data_engine.data_count == 1

    def test_handle_bar_sends_to_data_engine(self):
        # Arrange
        bar = TestStubs.bar_5decimal()

        # Act
        self.client._handle_data_py(bar)

        # Assert
        assert self.data_engine.data_count == 1

    def test_handle_quote_ticks_sends_to_data_engine(self):
        # Arrange
        # Act
        self.client._handle_quote_ticks_py(AUDUSD_SIM.id, [], self.uuid_factory.generate())

        # Assert
        assert self.data_engine.response_count == 1

    def test_handle_trade_ticks_sends_to_data_engine(self):
        # Arrange
        # Act
        self.client._handle_trade_ticks_py(AUDUSD_SIM.id, [], self.uuid_factory.generate())

        # Assert
        assert self.data_engine.response_count == 1

    def test_handle_bars_sends_to_data_engine(self):
        # Arrange
        # Act
        self.client._handle_bars_py(
            TestStubs.bartype_gbpusd_1sec_mid(),
            [],
            None,
            self.uuid_factory.generate(),
        )

        # Assert
        assert self.data_engine.response_count == 1
