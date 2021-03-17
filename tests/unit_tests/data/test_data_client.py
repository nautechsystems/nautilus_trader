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

import unittest

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import TestLogger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.data.client import DataClient
from nautilus_trader.data.client import MarketDataClient
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.model.bar import Bar
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import GenericData
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import TradeMatchId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.order_book_old import OrderBook
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.tick import TradeTick
from nautilus_trader.trading.portfolio import Portfolio
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH


SIM = Venue("SIM")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD", SIM)
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class DataClientTests(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = TestLogger(self.clock)

        self.portfolio = Portfolio(
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine = DataEngine(
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
        )

        self.venue = Venue("SIM")

        self.client = DataClient(
            name="TEST_PROVIDER",
            engine=self.data_engine,
            clock=self.clock,
            logger=self.logger,
        )

    def test_connect_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(NotImplementedError, self.client.connect)

    def test_disconnect_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(NotImplementedError, self.client.disconnect)

    def test_reset_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(NotImplementedError, self.client.reset)

    def test_dispose_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(NotImplementedError, self.client.dispose)

    def test_subscribe_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(NotImplementedError, self.client.subscribe, DataType(str))

    def test_unsubscribe_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(NotImplementedError, self.client.unsubscribe, DataType(str))

    def test_request_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            NotImplementedError,
            self.client.request,
            DataType(str),
            self.uuid_factory.generate(),
        )

    def test_handle_data_sends_to_data_engine(self):
        # Arrange
        data_type = DataType(str, {"Type": "NEWS_WIRE"})
        data = GenericData(data_type, "Some news headline", UNIX_EPOCH)

        # Act
        self.client._handle_data_py(data)

        # Assert
        self.assertEqual(1, self.data_engine.data_count)

    def test_handle_data_response_sends_to_data_engine(self):
        # Arrange
        data_type = DataType(str, {"Type": "ECONOMIC_DATA", "topic": "unemployment"})
        data = GenericData(data_type, "may 2020, 6.9%", UNIX_EPOCH)

        # Act
        self.client._handle_data_response_py(data, self.uuid_factory.generate())

        # Assert
        self.assertEqual(1, self.data_engine.response_count)


class MarketDataClientTests(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = TestLogger(self.clock)

        self.portfolio = Portfolio(
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine = DataEngine(
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
        )

        self.venue = Venue("SIM")

        self.client = MarketDataClient(
            name=self.venue.value,
            engine=self.data_engine,
            clock=self.clock,
            logger=self.logger,
        )

    def test_connect_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(NotImplementedError, self.client.connect)

    def test_disconnect_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(NotImplementedError, self.client.disconnect)

    def test_reset_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(NotImplementedError, self.client.reset)

    def test_dispose_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(NotImplementedError, self.client.dispose)

    def test_subscribe_instrument_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            NotImplementedError, self.client.subscribe_instrument, AUDUSD_SIM.id
        )

    def test_subscribe_order_book_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            NotImplementedError, self.client.subscribe_order_book, AUDUSD_SIM.id, 2, 0
        )

    def test_subscribe_quote_ticks_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            NotImplementedError, self.client.subscribe_quote_ticks, AUDUSD_SIM.id
        )

    def test_subscribe_trade_ticks_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            NotImplementedError, self.client.subscribe_trade_ticks, AUDUSD_SIM.id
        )

    def test_subscribe_bars_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            NotImplementedError,
            self.client.subscribe_bars,
            TestStubs.bartype_gbpusd_1sec_mid(),
        )

    def test_unsubscribe_instrument_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            NotImplementedError, self.client.unsubscribe_instrument, AUDUSD_SIM.id
        )

    def test_unsubscribe_order_book_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            NotImplementedError, self.client.unsubscribe_order_book, AUDUSD_SIM.id
        )

    def test_unsubscribe_quote_ticks_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            NotImplementedError, self.client.unsubscribe_quote_ticks, AUDUSD_SIM.id
        )

    def test_unsubscribe_trade_ticks_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            NotImplementedError, self.client.unsubscribe_trade_ticks, AUDUSD_SIM.id
        )

    def test_unsubscribe_bars_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            NotImplementedError,
            self.client.unsubscribe_bars,
            TestStubs.bartype_gbpusd_1sec_mid(),
        )

    def test_request_instrument_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            NotImplementedError, self.client.request_instrument, None, None
        )

    def test_request_instruments_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(NotImplementedError, self.client.request_instruments, None)

    def test_request_quote_ticks_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            NotImplementedError,
            self.client.request_quote_ticks,
            None,
            None,
            None,
            0,
            None,
        )

    def test_request_trade_ticks_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            NotImplementedError,
            self.client.request_trade_ticks,
            None,
            None,
            None,
            0,
            None,
        )

    def test_request_bars_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            NotImplementedError, self.client.request_bars, None, None, None, 0, None
        )

    def test_unavailable_methods_when_none_given_returns_empty_list(self):
        # Arrange
        # Act
        result = self.client.unavailable_methods()

        # Assert
        self.assertEqual([], result)

    def test_handle_instrument_sends_to_data_engine(self):
        # Arrange
        # Act
        self.client._handle_instrument_py(AUDUSD_SIM)

        # Assert
        self.assertEqual(1, self.data_engine.data_count)

    def test_handle_order_book_sends_to_data_engine(self):
        # Arrange
        order_book = OrderBook(
            instrument_id=ETHUSDT_BINANCE.id,
            level=2,
            depth=25,
            price_precision=2,
            size_precision=5,
            bids=[],
            asks=[],
            update_id=0,
            timestamp=0,
        )

        # Act
        self.client._handle_order_book_py(order_book)

        # Assert
        self.assertEqual(1, self.data_engine.data_count)

    def test_handle_quote_tick_sends_to_data_engine(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.id,
            Price("1.00050"),
            Price("1.00048"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        # Act
        self.client._handle_quote_tick_py(tick)

        # Assert
        self.assertEqual(1, self.data_engine.data_count)

    def test_handle_trade_tick_sends_to_data_engine(self):
        # Arrange
        tick = TradeTick(
            AUDUSD_SIM.id,
            Price("1.00050"),
            Quantity(1),
            OrderSide.BUY,
            TradeMatchId("123456"),
            UNIX_EPOCH,
        )

        # Act
        self.client._handle_trade_tick_py(tick)

        # Assert
        self.assertEqual(1, self.data_engine.data_count)

    def test_handle_bar_sends_to_data_engine(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()

        bar = Bar(
            Price("1.00001"),
            Price("1.00004"),
            Price("1.00002"),
            Price("1.00003"),
            Quantity(100000),
            UNIX_EPOCH,
        )

        # Act
        self.client._handle_bar_py(bar_type, bar)

        # Assert
        self.assertEqual(1, self.data_engine.data_count)

    def test_handle_instruments_sends_to_data_engine(self):
        # Arrange
        # Act
        self.client._handle_instruments_py([], self.uuid_factory.generate())

        # Assert
        self.assertEqual(1, self.data_engine.response_count)

    def test_handle_quote_ticks_sends_to_data_engine(self):
        # Arrange
        # Act
        self.client._handle_quote_ticks_py(
            AUDUSD_SIM.id, [], self.uuid_factory.generate()
        )

        # Assert
        self.assertEqual(1, self.data_engine.response_count)

    def test_handle_trade_ticks_sends_to_data_engine(self):
        # Arrange
        # Act
        self.client._handle_trade_ticks_py(
            AUDUSD_SIM.id, [], self.uuid_factory.generate()
        )

        # Assert
        self.assertEqual(1, self.data_engine.response_count)

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
        self.assertEqual(1, self.data_engine.response_count)
