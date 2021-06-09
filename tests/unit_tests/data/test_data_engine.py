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

from nautilus_trader.backtest.data_client import BacktestMarketDataClient
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.core.fsm import InvalidStateTrigger
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.data.messages import DataCommand
from nautilus_trader.data.messages import DataRequest
from nautilus_trader.data.messages import DataResponse
from nautilus_trader.data.messages import Subscribe
from nautilus_trader.data.messages import Unsubscribe
from nautilus_trader.model.bar import Bar
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.bar import BarType
from nautilus_trader.model.data import Data
from nautilus_trader.model.data import DataType
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OrderBookLevel
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeMatchId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.book import L2OrderBook
from nautilus_trader.model.orderbook.book import OrderBook
from nautilus_trader.model.orderbook.book import OrderBookData
from nautilus_trader.model.orderbook.book import OrderBookDeltas
from nautilus_trader.model.orderbook.book import OrderBookSnapshot
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.tick import TradeTick
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.mocks import MockMarketDataClient
from tests.test_kit.mocks import ObjectStorer
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


BITMEX = Venue("BITMEX")
BINANCE = Venue("BINANCE")
XBTUSD_BITMEX = TestInstrumentProvider.xbtusd_bitmex()
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class DataEngineTests(unittest.TestCase):
    def setUp(self):
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

        self.binance_client = BacktestMarketDataClient(
            client_id=ClientId(BINANCE.value),
            engine=self.data_engine,
            clock=self.clock,
            logger=self.logger,
        )

        self.bitmex_client = BacktestMarketDataClient(
            client_id=ClientId(BITMEX.value),
            engine=self.data_engine,
            clock=self.clock,
            logger=self.logger,
        )

        self.quandl = MockMarketDataClient(
            client_id=ClientId("QUANDL"),
            engine=self.data_engine,
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine.process(BTCUSDT_BINANCE)
        self.data_engine.process(ETHUSDT_BINANCE)
        self.data_engine.process(XBTUSD_BITMEX)

    def test_registered_venues(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual([], self.data_engine.registered_clients)

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

    def test_register_client_successfully_adds_client(self):
        # Arrange
        # Act
        self.data_engine.register_client(self.binance_client)

        # Assert
        self.assertIn(ClientId(BINANCE.value), self.data_engine.registered_clients)

    def test_deregister_client_successfully_removes_client(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        # Act
        self.data_engine.deregister_client(self.binance_client)

        # Assert
        self.assertNotIn(BINANCE.value, self.data_engine.registered_clients)

    def test_register_strategy_successfully_registered_with_strategy(self):
        # Arrange
        strategy = TradingStrategy("000")

        # Act
        strategy.register_data_engine(self.data_engine)

        # Assert
        self.assertEqual(self.data_engine.cache, strategy.cache)

    def test_reset(self):
        # Arrange
        # Act
        self.data_engine.reset()

        # Assert
        self.assertEqual(0, self.data_engine.command_count)
        self.assertEqual(0, self.data_engine.data_count)
        self.assertEqual(0, self.data_engine.request_count)
        self.assertEqual(0, self.data_engine.response_count)

    def test_stop_and_resume(self):
        # Arrange
        self.data_engine.start()

        # Act
        self.data_engine.stop()
        self.data_engine.resume()
        self.data_engine.stop()
        self.data_engine.reset()

        # Assert
        self.assertEqual(0, self.data_engine.command_count)
        self.assertEqual(0, self.data_engine.data_count)
        self.assertEqual(0, self.data_engine.request_count)
        self.assertEqual(0, self.data_engine.response_count)

    def test_dispose(self):
        # Arrange
        self.data_engine.reset()

        # Act
        self.data_engine.dispose()

        # Assert
        self.assertEqual(0, self.data_engine.command_count)
        self.assertEqual(0, self.data_engine.data_count)
        self.assertEqual(0, self.data_engine.request_count)
        self.assertEqual(0, self.data_engine.response_count)

    def test_check_connected_when_client_disconnected_returns_false(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.data_engine.register_client(self.bitmex_client)

        self.binance_client.disconnect()
        self.bitmex_client.disconnect()

        # Act
        result = self.data_engine.check_connected()

        # Assert
        self.assertFalse(result)

    def test_check_connected_when_client_connected_returns_true(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.data_engine.register_client(self.bitmex_client)

        self.binance_client.connect()
        self.bitmex_client.connect()

        # Act
        result = self.data_engine.check_connected()

        # Assert
        self.assertTrue(result)

    def test_check_disconnected_when_client_disconnected_returns_true(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.data_engine.register_client(self.bitmex_client)

        # Act
        result = self.data_engine.check_disconnected()

        # Assert
        self.assertTrue(result)

    def test_check_disconnected_when_client_connected_returns_false(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.data_engine.register_client(self.bitmex_client)

        self.binance_client.connect()
        self.bitmex_client.connect()

        # Act
        result = self.data_engine.check_disconnected()

        # Assert
        self.assertFalse(result)

    def test_reset_when_already_disposed_raises_invalid_state_trigger(self):
        # Arrange
        self.data_engine.dispose()

        # Act
        # Assert
        self.assertRaises(InvalidStateTrigger, self.data_engine.reset)

    def test_dispose_when_already_disposed_raises_invalid_state_trigger(self):
        # Arrange
        self.data_engine.dispose()

        # Act
        # Assert
        self.assertRaises(InvalidStateTrigger, self.data_engine.dispose)

    def test_execute_unrecognized_message_logs_and_does_nothing(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        # Bogus message
        command = DataCommand(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(str),
            handler=[].append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(command)

        # Assert
        self.assertEqual(1, self.data_engine.command_count)

    def test_send_request_when_no_data_clients_registered_does_nothing(self):
        # Arrange
        handler = []
        request = DataRequest(
            client_id=ClientId("RANDOM"),
            data_type=DataType(
                QuoteTick,
                metadata={
                    "InstrumentId": InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
                    "FromDateTime": None,
                    "ToDateTime": None,
                    "Limit": 1000,
                },
            ),
            callback=handler.append,
            request_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.send(request)

        # Assert
        self.assertEqual(1, self.data_engine.request_count)

    def test_send_data_request_when_data_type_unrecognized_logs_and_does_nothing(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        handler = []
        request = DataRequest(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                str,
                metadata={  # str data type is invalid
                    "InstrumentId": InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
                    "FromDateTime": None,
                    "ToDateTime": None,
                    "Limit": 1000,
                },
            ),
            callback=handler.append,
            request_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.send(request)

        # Assert
        self.assertEqual(1, self.data_engine.request_count)

    def test_send_data_request_with_duplicate_ids_logs_and_does_not_handle_second(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.data_engine.start()

        handler = []
        uuid = self.uuid_factory.generate()  # We'll use this as a duplicate

        request1 = DataRequest(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                QuoteTick,
                metadata={  # str data type is invalid
                    "InstrumentId": InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
                    "FromDateTime": None,
                    "ToDateTime": None,
                    "Limit": 1000,
                },
            ),
            callback=handler.append,
            request_id=uuid,  # Duplicate
            timestamp_ns=self.clock.timestamp_ns(),
        )

        request2 = DataRequest(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                QuoteTick,
                metadata={  # str data type is invalid
                    "InstrumentId": InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
                    "FromDateTime": None,
                    "ToDateTime": None,
                    "Limit": 1000,
                },
            ),
            callback=handler.append,
            request_id=uuid,  # Duplicate
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.send(request1)
        self.data_engine.send(request2)

        # Assert
        self.assertEqual(2, self.data_engine.request_count)

    def test_execute_subscribe_when_data_type_unrecognized_logs_and_does_nothing(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(str),  # str data type is invalid
            handler=[].append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        self.assertEqual(1, self.data_engine.command_count)

    def test_execute_subscribe_when_already_subscribed_does_not_add_and_logs(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                QuoteTick, metadata={"InstrumentId": ETHUSDT_BINANCE.id}
            ),
            handler=[].append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)
        self.data_engine.execute(subscribe)

        # Assert
        self.assertEqual(2, self.data_engine.command_count)

    def test_execute_subscribe_custom_data(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.data_engine.register_client(self.quandl)
        self.binance_client.connect()

        subscribe = Subscribe(
            client_id=ClientId("QUANDL"),
            data_type=DataType(str, metadata={"Type": "news"}),
            handler=[].append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        self.assertEqual(1, self.data_engine.command_count)
        self.assertEqual(["subscribe"], self.quandl.calls)

    def test_execute_unsubscribe_custom_data(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.data_engine.register_client(self.quandl)
        self.binance_client.connect()

        handler = []
        subscribe = Subscribe(
            client_id=ClientId("QUANDL"),
            data_type=DataType(str, metadata={"Type": "news"}),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        unsubscribe = Unsubscribe(
            client_id=ClientId("QUANDL"),
            data_type=DataType(str, metadata={"Type": "news"}),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        self.assertEqual(2, self.data_engine.command_count)
        self.assertEqual(["subscribe", "unsubscribe"], self.quandl.calls)

    def test_execute_unsubscribe_when_data_type_unrecognized_logs_and_does_nothing(
        self,
    ):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        handler = []
        unsubscribe = Unsubscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(type(str)),  # str data type is invalid
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        self.assertEqual(1, self.data_engine.command_count)

    def test_execute_unsubscribe_when_not_subscribed_logs_and_does_nothing(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler = []
        unsubscribe = Unsubscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                type(QuoteTick), metadata={"InstrumentId": ETHUSDT_BINANCE.id}
            ),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        self.assertEqual(1, self.data_engine.command_count)

    def test_receive_response_when_no_data_clients_registered_does_nothing(self):
        # Arrange
        response = DataResponse(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(QuoteTick),
            data=[],
            correlation_id=self.uuid_factory.generate(),
            response_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.receive(response)

        # Assert
        self.assertEqual(1, self.data_engine.response_count)

    def test_process_unrecognized_data_type_logs_and_does_nothing(self):
        # Arrange
        data = Data(0, 0)

        # Act
        self.data_engine.process(data)  # Invalid

        # Assert
        self.assertEqual(4, self.data_engine.data_count)

    def test_process_data_places_data_on_queue(self):
        # Arrange
        tick = TestStubs.trade_tick_5decimal()

        # Act
        self.data_engine.process(tick)

        # Assert
        self.assertEqual(4, self.data_engine.data_count)

    def test_execute_subscribe_instrument_then_adds_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                Instrument, metadata={"InstrumentId": ETHUSDT_BINANCE.id}
            ),
            handler=[].append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        self.assertEqual([ETHUSDT_BINANCE.id], self.data_engine.subscribed_instruments)

    def test_execute_unsubscribe_instrument_then_removes_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler = []
        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                Instrument, metadata={"InstrumentId": ETHUSDT_BINANCE.id}
            ),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        unsubscribe = Unsubscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                Instrument, metadata={"InstrumentId": ETHUSDT_BINANCE.id}
            ),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        self.assertEqual([], self.data_engine.subscribed_instruments)

    def test_process_instrument_when_subscriber_then_sends_to_registered_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler = []
        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                Instrument, metadata={"InstrumentId": ETHUSDT_BINANCE.id}
            ),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        # Act
        self.data_engine.process(ETHUSDT_BINANCE)

        # Assert
        self.assertEqual([ETHUSDT_BINANCE], handler)

    def test_process_instrument_when_subscribers_then_sends_to_registered_handlers(
        self,
    ):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler1 = []
        subscribe1 = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                Instrument, metadata={"InstrumentId": ETHUSDT_BINANCE.id}
            ),
            handler=handler1.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        handler2 = []
        subscribe2 = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                Instrument, metadata={"InstrumentId": ETHUSDT_BINANCE.id}
            ),
            handler=handler2.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe1)
        self.data_engine.execute(subscribe2)

        # Act
        self.data_engine.process(ETHUSDT_BINANCE)

        # Assert
        self.assertEqual([ETHUSDT_BINANCE.id], self.data_engine.subscribed_instruments)
        self.assertEqual([ETHUSDT_BINANCE], handler1)
        self.assertEqual([ETHUSDT_BINANCE], handler2)

    def test_execute_subscribe_order_book_stream_then_adds_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                OrderBook,
                metadata={
                    "InstrumentId": ETHUSDT_BINANCE.id,
                    "Level": 2,
                    "Depth": 10,
                    "Interval": 0,
                },
            ),
            handler=[].append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        self.assertEqual([ETHUSDT_BINANCE.id], self.data_engine.subscribed_order_books)

    def test_execute_subscribe_order_book_data_then_adds_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                OrderBookData,
                metadata={
                    "InstrumentId": ETHUSDT_BINANCE.id,
                    "Level": 2,
                    "Depth": 10,
                    "Interval": 0,
                },
            ),
            handler=[].append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        self.assertEqual(
            [ETHUSDT_BINANCE.id], self.data_engine.subscribed_order_book_data
        )

    def test_execute_subscribe_order_book_intervals_then_adds_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                OrderBook,
                metadata={
                    "InstrumentId": ETHUSDT_BINANCE.id,
                    "Level": 2,
                    "Depth": 25,
                    "Interval": 10,
                },
            ),
            handler=[].append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        self.assertEqual([ETHUSDT_BINANCE.id], self.data_engine.subscribed_order_books)

    def test_execute_unsubscribe_order_book_stream_then_removes_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler = []
        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                OrderBook,
                metadata={
                    "InstrumentId": ETHUSDT_BINANCE.id,
                    "Level": 2,
                    "Depth": 25,
                    "Interval": 0,
                },
            ),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        unsubscribe = Unsubscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                OrderBook,
                metadata={
                    "InstrumentId": ETHUSDT_BINANCE.id,
                    "Interval": 0,
                },
            ),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        self.assertEqual([], self.data_engine.subscribed_order_books)

    def test_execute_unsubscribe_order_book_data_then_removes_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler = []
        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                OrderBookData,
                metadata={
                    "InstrumentId": ETHUSDT_BINANCE.id,
                    "Level": 2,
                    "Depth": 25,
                    "Interval": 0,
                },
            ),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        unsubscribe = Unsubscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                OrderBookData,
                metadata={
                    "InstrumentId": ETHUSDT_BINANCE.id,
                    "Interval": 0,
                },
            ),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        self.assertEqual([], self.data_engine.subscribed_order_books)

    def test_execute_unsubscribe_order_book_interval_then_removes_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler = []
        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                OrderBook,
                metadata={
                    "InstrumentId": ETHUSDT_BINANCE.id,
                    "Level": 2,
                    "Depth": 25,
                    "Interval": 10,
                },
            ),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        unsubscribe = Unsubscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                OrderBook,
                metadata={
                    "InstrumentId": ETHUSDT_BINANCE.id,
                    "Interval": 10,
                },
            ),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        self.assertEqual([], self.data_engine.subscribed_order_books)

    def test_process_order_book_snapshot_when_one_subscriber_then_sends_to_registered_handler(
        self,
    ):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        self.data_engine.process(
            ETHUSDT_BINANCE
        )  # <-- add necessary instrument for test

        handler = []
        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                OrderBook,
                {
                    "InstrumentId": ETHUSDT_BINANCE.id,
                    "Level": OrderBookLevel.L2,
                    "Depth": 25,
                    "Interval": 0,  # Streaming
                },
            ),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        snapshot = OrderBookSnapshot(
            instrument_id=ETHUSDT_BINANCE.id,
            level=OrderBookLevel.L2,
            bids=[[1000, 1]],
            asks=[[1001, 1]],
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        # Act
        self.data_engine.process(snapshot)

        # Assert
        assert self.data_engine.subscribed_order_books == [ETHUSDT_BINANCE.id]
        assert handler[0].instrument_id == ETHUSDT_BINANCE.id
        assert type(handler[0]) == L2OrderBook

    def test_process_order_book_ops_then_sends_to_registered_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        self.data_engine.process(
            ETHUSDT_BINANCE
        )  # <-- add necessary instrument for test

        handler = []
        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                OrderBook,
                {
                    "InstrumentId": ETHUSDT_BINANCE.id,
                    "Level": OrderBookLevel.L2,
                    "Depth": 25,
                    "Interval": 0,  # Streaming
                },
            ),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        deltas = OrderBookDeltas(
            instrument_id=ETHUSDT_BINANCE.id,
            level=OrderBookLevel.L2,
            deltas=[],
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        # Act
        self.data_engine.process(deltas)

        # Assert
        assert self.data_engine.subscribed_order_books == [ETHUSDT_BINANCE.id]
        assert handler[0].instrument_id == ETHUSDT_BINANCE.id
        assert type(handler[0]) == L2OrderBook

    def test_process_order_book_when_multiple_subscribers_then_sends_to_registered_handlers(
        self,
    ):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        self.data_engine.process(
            ETHUSDT_BINANCE
        )  # <-- add necessary instrument for test

        handler1 = []
        subscribe1 = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                OrderBook,
                {
                    "InstrumentId": ETHUSDT_BINANCE.id,
                    "Level": OrderBookLevel.L2,
                    "Depth": 25,
                    "Interval": 0,  # Streaming
                },
            ),
            handler=handler1.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        handler2 = []
        subscribe2 = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                OrderBook,
                {
                    "InstrumentId": ETHUSDT_BINANCE.id,
                    "Level": OrderBookLevel.L2,
                    "Depth": 25,
                    "Interval": 0,  # Streaming
                },
            ),
            handler=handler2.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe1)
        self.data_engine.execute(subscribe2)

        snapshot = OrderBookSnapshot(
            instrument_id=ETHUSDT_BINANCE.id,
            level=OrderBookLevel.L2,
            bids=[[1000, 1]],
            asks=[[1001, 1]],
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        # Act
        self.data_engine.process(snapshot)

        # Assert
        cached_book = self.data_engine.cache.order_book(ETHUSDT_BINANCE.id)
        assert self.data_engine.subscribed_order_books == [ETHUSDT_BINANCE.id]
        assert type(cached_book) == L2OrderBook
        assert cached_book.instrument_id == ETHUSDT_BINANCE.id
        assert handler1[0] == cached_book
        assert handler2[0] == cached_book

    def test_execute_subscribe_for_quote_ticks(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler = []
        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                QuoteTick, metadata={"InstrumentId": ETHUSDT_BINANCE.id}
            ),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        # Assert
        self.assertEqual([ETHUSDT_BINANCE.id], self.data_engine.subscribed_quote_ticks)

    def test_execute_unsubscribe_for_quote_ticks(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler = []
        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                QuoteTick, metadata={"InstrumentId": ETHUSDT_BINANCE.id}
            ),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        unsubscribe = Unsubscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                QuoteTick, metadata={"InstrumentId": ETHUSDT_BINANCE.id}
            ),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        self.assertEqual([], self.data_engine.subscribed_quote_ticks)

    def test_process_quote_tick_when_subscriber_then_sends_to_registered_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler = []
        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                QuoteTick, metadata={"InstrumentId": ETHUSDT_BINANCE.id}
            ),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        tick = QuoteTick(
            ETHUSDT_BINANCE.id,
            Price.from_str("100.003"),
            Price.from_str("100.003"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        self.assertEqual([ETHUSDT_BINANCE.id], self.data_engine.subscribed_quote_ticks)
        self.assertEqual([tick], handler)

    def test_process_quote_tick_when_subscribers_then_sends_to_registered_handlers(
        self,
    ):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler1 = []
        subscribe1 = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                QuoteTick, metadata={"InstrumentId": ETHUSDT_BINANCE.id}
            ),
            handler=handler1.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        handler2 = []
        subscribe2 = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                QuoteTick, metadata={"InstrumentId": ETHUSDT_BINANCE.id}
            ),
            handler=handler2.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe1)
        self.data_engine.execute(subscribe2)

        tick = QuoteTick(
            ETHUSDT_BINANCE.id,
            Price.from_str("100.003"),
            Price.from_str("100.003"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        self.assertEqual([ETHUSDT_BINANCE.id], self.data_engine.subscribed_quote_ticks)
        self.assertEqual([tick], handler1)
        self.assertEqual([tick], handler2)

    def test_subscribe_trade_tick_then_subscribes(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler = []
        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                TradeTick, metadata={"InstrumentId": ETHUSDT_BINANCE.id}
            ),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        self.assertEqual([ETHUSDT_BINANCE.id], self.data_engine.subscribed_trade_ticks)

    def test_unsubscribe_trade_tick_then_unsubscribes(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler = []
        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                TradeTick, metadata={"InstrumentId": ETHUSDT_BINANCE.id}
            ),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        unsubscribe = Unsubscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                TradeTick, metadata={"InstrumentId": ETHUSDT_BINANCE.id}
            ),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        self.assertEqual([], self.data_engine.subscribed_trade_ticks)

    def test_process_trade_tick_when_subscriber_then_sends_to_registered_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler = []
        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                TradeTick, metadata={"InstrumentId": ETHUSDT_BINANCE.id}
            ),
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        tick = TradeTick(
            ETHUSDT_BINANCE.id,
            Price.from_str("1050.00000"),
            Quantity.from_int(100),
            AggressorSide.BUY,
            TradeMatchId("123456789"),
            0,
            0,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        self.assertEqual([tick], handler)

    def test_process_trade_tick_when_subscribers_then_sends_to_registered_handlers(
        self,
    ):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler1 = []
        subscribe1 = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                TradeTick, metadata={"InstrumentId": ETHUSDT_BINANCE.id}
            ),
            handler=handler1.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        handler2 = []
        subscribe2 = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                TradeTick, metadata={"InstrumentId": ETHUSDT_BINANCE.id}
            ),
            handler=handler2.append,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe1)
        self.data_engine.execute(subscribe2)

        tick = TradeTick(
            ETHUSDT_BINANCE.id,
            Price.from_str("1050.00000"),
            Quantity.from_int(100),
            AggressorSide.BUY,
            TradeMatchId("123456789"),
            0,
            0,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        self.assertEqual([tick], handler1)
        self.assertEqual([tick], handler2)

    def test_subscribe_bar_type_then_subscribes(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(ETHUSDT_BINANCE.id, bar_spec, internal_aggregation=True)

        handler = ObjectStorer()
        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(Bar, metadata={"BarType": bar_type}),
            handler=handler.store_2,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        self.assertEqual([bar_type], self.data_engine.subscribed_bars)

    def test_unsubscribe_bar_type_then_unsubscribes(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(ETHUSDT_BINANCE.id, bar_spec, internal_aggregation=True)

        handler = ObjectStorer()
        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(Bar, metadata={"BarType": bar_type}),
            handler=handler.store_2,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        unsubscribe = Unsubscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(Bar, metadata={"BarType": bar_type}),
            handler=handler.store_2,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        self.assertEqual([], self.data_engine.subscribed_bars)

    def test_process_bar_when_subscriber_then_sends_to_registered_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(ETHUSDT_BINANCE.id, bar_spec, internal_aggregation=True)

        handler = ObjectStorer()
        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(Bar, metadata={"BarType": bar_type}),
            handler=handler.store,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        bar = Bar(
            bar_type,
            Price.from_str("1051.00000"),
            Price.from_str("1055.00000"),
            Price.from_str("1050.00000"),
            Price.from_str("1052.00000"),
            Quantity.from_int(100),
            0,
            0,
        )

        # Act
        self.data_engine.process(bar)

        # Assert
        self.assertEqual([bar], handler.get_store())

    def test_process_bar_when_subscribers_then_sends_to_registered_handlers(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(ETHUSDT_BINANCE.id, bar_spec, internal_aggregation=True)

        handler1 = ObjectStorer()
        subscribe1 = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(Bar, metadata={"BarType": bar_type}),
            handler=handler1.store,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        handler2 = ObjectStorer()
        subscribe2 = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(Bar, metadata={"BarType": bar_type}),
            handler=handler2.store,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe1)
        self.data_engine.execute(subscribe2)

        bar = Bar(
            bar_type,
            Price.from_str("1051.00000"),
            Price.from_str("1055.00000"),
            Price.from_str("1050.00000"),
            Price.from_str("1052.00000"),
            Quantity.from_int(100),
            0,
            0,
        )

        # Act
        self.data_engine.process(bar)

        # Assert
        self.assertEqual([bar], handler1.get_store())
        self.assertEqual([bar], handler2.get_store())
