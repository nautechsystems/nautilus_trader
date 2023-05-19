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

import sys

import pandas as pd

from nautilus_trader.backtest.data_client import BacktestMarketDataClient
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.data import Data
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.data.engine import DataEngineConfig
from nautilus_trader.data.messages import DataCommand
from nautilus_trader.data.messages import DataRequest
from nautilus_trader.data.messages import DataResponse
from nautilus_trader.data.messages import Subscribe
from nautilus_trader.data.messages import Unsubscribe
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.bar import BarSpecification
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.data.book import OrderBookDelta
from nautilus_trader.model.data.book import OrderBookDeltas
from nautilus_trader.model.data.book import OrderBookSnapshot
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.ticker import Ticker
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook import L2OrderBook
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.external.core import write_objects
from nautilus_trader.persistence.external.readers import CSVReader
from nautilus_trader.persistence.wranglers import BarDataWrangler
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.mocks.data import data_catalog_setup
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs import UNIX_EPOCH
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.filters import NewsEvent
from tests import TEST_DATA_DIR
from tests.unit_tests.portfolio.test_portfolio import BETFAIR


BITMEX = Venue("BITMEX")
BINANCE = Venue("BINANCE")
XBTUSD_BITMEX = TestInstrumentProvider.xbtusd_bitmex()
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()
BETFAIR_INSTRUMENT = TestInstrumentProvider.betting_instrument()


class TestDataEngine:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.logger = Logger(
            clock=self.clock,
            level_stdout=LogLevel.DEBUG,
            bypass=True,
        )

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

        config = DataEngineConfig(
            validate_data_sequence=True,
            debug=True,
        )
        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=config,
        )

        self.binance_client = BacktestMarketDataClient(
            client_id=ClientId(BINANCE.value),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.bitmex_client = BacktestMarketDataClient(
            client_id=ClientId(BITMEX.value),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.quandl = BacktestMarketDataClient(
            client_id=ClientId("QUANDL"),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.betfair = BacktestMarketDataClient(
            client_id=ClientId("BETFAIR"),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine.process(BTCUSDT_BINANCE)
        self.data_engine.process(ETHUSDT_BINANCE)
        self.data_engine.process(XBTUSD_BITMEX)

    def test_registered_venues(self):
        # Arrange, Act, Assert
        assert self.data_engine.registered_clients == []

    def test_subscribed_instruments_when_nothing_subscribed_returns_empty_list(self):
        # Arrange, Act, Assert
        assert self.data_engine.subscribed_instruments() == []

    def test_subscribed_quote_ticks_when_nothing_subscribed_returns_empty_list(self):
        # Arrange, Act, Assert
        assert self.data_engine.subscribed_quote_ticks() == []

    def test_subscribed_trade_ticks_when_nothing_subscribed_returns_empty_list(self):
        # Arrange, Act, Assert
        assert self.data_engine.subscribed_trade_ticks() == []

    def test_subscribed_bars_when_nothing_subscribed_returns_empty_list(self):
        # Arrange, Act, Assert
        assert self.data_engine.subscribed_bars() == []

    def test_register_client_successfully_adds_client(self):
        # Arrange, Act
        self.data_engine.register_client(self.binance_client)

        # Assert
        assert ClientId(BINANCE.value) in self.data_engine.registered_clients

    def test_deregister_client_successfully_removes_client(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        # Act
        self.data_engine.deregister_client(self.binance_client)

        # Assert
        assert BINANCE.value not in self.data_engine.registered_clients

    def test_reset(self):
        # Arrange, Act
        self.data_engine.reset()

        # Assert
        assert self.data_engine.command_count == 0
        assert self.data_engine.data_count == 0
        assert self.data_engine.request_count == 0
        assert self.data_engine.response_count == 0

    def test_stop_and_resume(self):
        # Arrange
        self.data_engine.start()

        # Act
        self.data_engine.stop()
        self.data_engine.resume()
        self.data_engine.stop()
        self.data_engine.reset()

        # Assert
        assert self.data_engine.command_count == 0
        assert self.data_engine.data_count == 0
        assert self.data_engine.request_count == 0
        assert self.data_engine.response_count == 0

    def test_dispose(self):
        # Arrange
        self.data_engine.reset()

        # Act
        self.data_engine.dispose()

        # Assert
        assert self.data_engine.command_count == 0
        assert self.data_engine.data_count == 0
        assert self.data_engine.request_count == 0
        assert self.data_engine.response_count == 0

    def test_check_connected_when_client_disconnected_returns_false(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.data_engine.register_client(self.bitmex_client)
        self.binance_client.start()
        self.bitmex_client.start()

        self.binance_client.stop()
        self.bitmex_client.stop()

        # Act
        result = self.data_engine.check_connected()

        # Assert
        assert not result

    def test_check_connected_when_client_connected_returns_true(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.data_engine.register_client(self.bitmex_client)

        self.binance_client.start()
        self.bitmex_client.start()

        # Act
        result = self.data_engine.check_connected()

        # Assert
        assert result

    def test_check_disconnected_when_client_disconnected_returns_true(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.data_engine.register_client(self.bitmex_client)

        # Act
        result = self.data_engine.check_disconnected()

        # Assert
        assert result

    def test_check_disconnected_when_client_connected_returns_false(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.data_engine.register_client(self.bitmex_client)

        self.binance_client.start()
        self.bitmex_client.start()

        # Act
        result = self.data_engine.check_disconnected()

        # Assert
        assert not result

    def test_execute_unrecognized_message_logs_and_does_nothing(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        # Bogus message
        command = DataCommand(
            client_id=None,
            venue=BINANCE,
            data_type=DataType(Data),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(command)

        # Assert
        assert self.data_engine.command_count == 1

    def test_send_request_when_no_data_clients_registered_does_nothing(self):
        # Arrange
        handler = []
        request = DataRequest(
            client_id=ClientId("RANDOM"),
            venue=None,
            data_type=DataType(
                QuoteTick,
                metadata={
                    "instrument_id": InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
                    "start": None,
                    "end": None,
                    "limit": 1000,
                },
            ),
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.request(request)

        # Assert
        assert self.data_engine.request_count == 1

    def test_send_data_request_when_data_type_unrecognized_logs_and_does_nothing(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        handler = []
        request = DataRequest(
            client_id=None,
            venue=BINANCE,
            data_type=DataType(
                Data,
                metadata={  # str data type is invalid
                    "instrument_id": InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
                    "start": None,
                    "end": None,
                    "limit": 1000,
                },
            ),
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.request(request)

        # Assert
        assert self.data_engine.request_count == 1

    def test_send_data_request_with_duplicate_ids_logs_and_does_not_handle_second(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.data_engine.start()

        handler = []
        uuid = UUID4()  # We'll use this as a duplicate

        request1 = DataRequest(
            client_id=None,
            venue=BINANCE,
            data_type=DataType(
                QuoteTick,
                metadata={  # str data type is invalid
                    "instrument_id": InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
                    "start": None,
                    "end": None,
                    "limit": 1000,
                },
            ),
            callback=handler.append,
            request_id=uuid,  # Duplicate
            ts_init=self.clock.timestamp_ns(),
        )

        request2 = DataRequest(
            client_id=None,
            venue=BINANCE,
            data_type=DataType(
                QuoteTick,
                metadata={  # str data type is invalid
                    "instrument_id": InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
                    "start": None,
                    "end": None,
                    "limit": 1000,
                },
            ),
            callback=handler.append,
            request_id=uuid,  # Duplicate
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.request(request1)
        self.data_engine.request(request2)

        # Assert
        assert self.data_engine.request_count == 2

    def test_execute_subscribe_when_data_type_not_implemented_logs_and_does_nothing(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        subscribe = Subscribe(
            client_id=None,
            venue=BINANCE,
            data_type=DataType(NewsEvent),  # NewsEvent data not recognized
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        assert self.data_engine.command_count == 1

    def test_execute_subscribe_custom_data_when_not_implemented(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.data_engine.register_client(self.quandl)
        self.binance_client.start()

        subscribe = Subscribe(
            client_id=ClientId("QUANDL"),
            venue=None,
            data_type=DataType(Data, metadata={"Type": "news"}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        assert self.data_engine.command_count == 1
        assert self.data_engine.subscribed_generic_data() == []

    def test_execute_unsubscribe_custom_data(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.data_engine.register_client(self.quandl)
        self.binance_client.start()

        data_type = DataType(Data, metadata={"Type": "news"})
        handler = []

        self.msgbus.subscribe(topic=f"data.{data_type.topic}", handler=handler.append)
        subscribe = Subscribe(
            client_id=ClientId("QUANDL"),
            venue=None,
            data_type=DataType(Data, metadata={"Type": "news"}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        self.msgbus.unsubscribe(topic=f"data.{data_type.topic}", handler=handler.append)
        unsubscribe = Unsubscribe(
            client_id=ClientId("QUANDL"),
            venue=None,
            data_type=DataType(Data, metadata={"Type": "news"}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.command_count == 2
        assert self.data_engine.subscribed_generic_data() == []

    def test_execute_unsubscribe_when_data_type_unrecognized_logs_and_does_nothing(
        self,
    ):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        unsubscribe = Unsubscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Data),  # str data type is invalid
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.command_count == 1

    def test_execute_unsubscribe_when_not_subscribed_logs_and_does_nothing(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        unsubscribe = Unsubscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(QuoteTick, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.command_count == 1

    def test_receive_response_when_no_data_clients_registered_does_nothing(self):
        # Arrange
        response = DataResponse(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(QuoteTick),
            data=[],
            correlation_id=UUID4(),
            response_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.response(response)

        # Assert
        assert self.data_engine.response_count == 1

    def test_process_unrecognized_data_type_logs_and_does_nothing(self):
        # Arrange
        data = Data(0, 0)

        # Act
        self.data_engine.process(data)  # Invalid

        # Assert
        assert self.data_engine.data_count == 4

    def test_process_data_places_data_on_queue(self):
        # Arrange
        tick = TestDataStubs.trade_tick_5decimal()

        # Act
        self.data_engine.process(tick)

        # Assert
        assert self.data_engine.data_count == 4

    def test_execute_subscribe_instruments_then_adds_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Instrument),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        assert self.data_engine.command_count == 1

    def test_execute_unsubscribe_instruments_then_removes_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Instrument),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        unsubscribe = Unsubscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Instrument),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.subscribed_instruments() == []

    def test_execute_subscribe_instrument_then_adds_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Instrument, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        assert self.data_engine.command_count == 1
        assert self.data_engine.subscribed_instruments() == [ETHUSDT_BINANCE.id]

    def test_execute_unsubscribe_instrument_then_removes_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Instrument, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        unsubscribe = Unsubscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Instrument, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.subscribed_instruments() == []

    def test_process_instrument_when_subscriber_then_sends_to_registered_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        handler = []
        self.msgbus.subscribe(topic="data.instrument.BINANCE.ETHUSDT", handler=handler.append)

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Instrument, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        # Act
        self.data_engine.process(ETHUSDT_BINANCE)

        # Assert
        assert handler == [ETHUSDT_BINANCE]

    def test_process_instrument_when_subscribers_then_sends_to_registered_handlers(
        self,
    ):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        handler1 = []
        handler2 = []
        self.msgbus.subscribe(topic="data.instrument.BINANCE.ETHUSDT", handler=handler1.append)
        self.msgbus.subscribe(topic="data.instrument.BINANCE.ETHUSDT", handler=handler2.append)

        subscribe1 = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Instrument, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        subscribe2 = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Instrument, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe1)
        self.data_engine.execute(subscribe2)

        # Act
        self.data_engine.process(ETHUSDT_BINANCE)

        # Assert
        assert handler1 == [ETHUSDT_BINANCE]
        assert handler2 == [ETHUSDT_BINANCE]

    def test_execute_subscribe_order_book_snapshots_then_adds_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(
                OrderBookSnapshot,
                metadata={
                    "instrument_id": ETHUSDT_BINANCE.id,
                    "book_type": 2,
                    "depth": 10,
                    "interval_ms": 1000,
                },
            ),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        assert self.data_engine.subscribed_order_book_deltas() == [ETHUSDT_BINANCE.id]

    def test_execute_subscribe_order_book_deltas_then_adds_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(
                OrderBookDelta,
                metadata={
                    "instrument_id": ETHUSDT_BINANCE.id,
                    "book_type": 2,
                    "depth": 10,
                    "interval_ms": 1000,
                },
            ),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        assert self.data_engine.subscribed_order_book_deltas() == [ETHUSDT_BINANCE.id]

    def test_execute_subscribe_order_book_intervals_then_adds_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(
                OrderBookSnapshot,
                metadata={
                    "instrument_id": ETHUSDT_BINANCE.id,
                    "book_type": 2,
                    "depth": 25,
                    "interval_ms": 1000,
                },
            ),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        assert self.data_engine.subscribed_order_book_deltas() == [ETHUSDT_BINANCE.id]

    def test_execute_unsubscribe_order_book_stream_then_removes_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(
                OrderBookSnapshot,
                metadata={
                    "instrument_id": ETHUSDT_BINANCE.id,
                    "book_type": 2,
                    "depth": 25,
                    "interval_ms": 1000,
                },
            ),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        unsubscribe = Unsubscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(
                OrderBookSnapshot,
                metadata={
                    "instrument_id": ETHUSDT_BINANCE.id,
                    "interval_ms": 1000,
                },
            ),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.subscribed_order_book_snapshots() == []

    def test_execute_unsubscribe_order_book_data_then_removes_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(
                OrderBookDelta,
                metadata={
                    "instrument_id": ETHUSDT_BINANCE.id,
                    "book_type": 2,
                    "depth": 25,
                    "interval_ms": 1000,
                },
            ),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        unsubscribe = Unsubscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(
                OrderBookDelta,
                metadata={
                    "instrument_id": ETHUSDT_BINANCE.id,
                    "interval_ms": 1000,
                },
            ),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.subscribed_order_book_snapshots() == []

    def test_execute_unsubscribe_order_book_interval_then_removes_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(
                OrderBookSnapshot,
                metadata={
                    "instrument_id": ETHUSDT_BINANCE.id,
                    "book_type": 2,
                    "depth": 25,
                    "interval_ms": 1000,
                },
            ),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        unsubscribe = Unsubscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(
                OrderBookSnapshot,
                metadata={
                    "instrument_id": ETHUSDT_BINANCE.id,
                    "interval_ms": 1000,
                },
            ),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.subscribed_order_book_snapshots() == []

    def test_order_book_snapshots_when_book_not_updated_does_not_send_(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        self.data_engine.process(ETHUSDT_BINANCE)  # <-- add necessary instrument for test

        handler = []
        self.msgbus.subscribe(
            topic="data.book.snapshots.BINANCE.ETHUSDT.1000",
            handler=handler.append,
        )

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(
                OrderBookSnapshot,
                {
                    "instrument_id": ETHUSDT_BINANCE.id,
                    "book_type": BookType.L2_MBP,
                    "depth": 20,
                    "interval_ms": 1000,  # Streaming
                },
            ),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        # Act
        events = self.clock.advance_time(1_000_000_000)
        events[0].handle()

        # Assert
        assert len(handler) == 0

    def test_process_order_book_snapshot_when_one_subscriber_then_sends_to_registered_handler(
        self,
    ):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        self.data_engine.process(ETHUSDT_BINANCE)  # <-- add necessary instrument for test

        handler = []
        self.msgbus.subscribe(
            topic="data.book.snapshots.BINANCE.ETHUSDT.1000",
            handler=handler.append,
        )

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(
                OrderBookSnapshot,
                {
                    "instrument_id": ETHUSDT_BINANCE.id,
                    "book_type": BookType.L2_MBP,
                    "depth": 25,
                    "interval_ms": 1000,  # Streaming
                },
            ),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        snapshot = OrderBookSnapshot(
            instrument_id=ETHUSDT_BINANCE.id,
            bids=[[1000, 1]],
            asks=[[1001, 1]],
            ts_event=1_000_000,
            ts_init=1_000_000,
        )

        # Act
        self.data_engine.process(snapshot)

        events = self.clock.advance_time(1_000_000_000)
        events[0].handle()

        # Assert
        assert isinstance(handler[0], L2OrderBook)

    def test_process_order_book_deltas_then_sends_to_registered_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        self.data_engine.process(ETHUSDT_BINANCE)  # <-- add necessary instrument for test

        handler = []
        self.msgbus.subscribe(topic="data.book.deltas.BINANCE.ETHUSDT", handler=handler.append)

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(
                OrderBookDelta,
                {
                    "instrument_id": ETHUSDT_BINANCE.id,
                    "book_type": BookType.L3_MBO,
                    "depth": 5,
                },
            ),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        deltas = OrderBookDeltas(
            instrument_id=ETHUSDT_BINANCE.id,
            deltas=[],
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.data_engine.process(deltas)

        # Assert
        assert handler[0].instrument_id == ETHUSDT_BINANCE.id
        assert isinstance(handler[0], OrderBookDeltas)

    def test_process_order_book_snapshots_when_multiple_subscribers_then_sends_to_registered_handlers(
        self,
    ):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        self.data_engine.process(ETHUSDT_BINANCE)  # <-- add necessary instrument for test

        handler1 = []
        handler2 = []
        self.msgbus.subscribe(
            topic="data.book.snapshots.BINANCE.ETHUSDT.1000",
            handler=handler1.append,
        )
        self.msgbus.subscribe(
            topic="data.book.snapshots.BINANCE.ETHUSDT.1000",
            handler=handler2.append,
        )

        subscribe1 = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(
                OrderBookSnapshot,
                {
                    "instrument_id": ETHUSDT_BINANCE.id,
                    "book_type": BookType.L2_MBP,
                    "depth": 25,
                    "interval_ms": 1000,
                },
            ),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        subscribe2 = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(
                OrderBookSnapshot,
                {
                    "instrument_id": ETHUSDT_BINANCE.id,
                    "book_type": BookType.L2_MBP,
                    "depth": 25,
                    "interval_ms": 1000,
                },
            ),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe1)
        self.data_engine.execute(subscribe2)

        snapshot = OrderBookSnapshot(
            instrument_id=ETHUSDT_BINANCE.id,
            bids=[[1000, 1]],
            asks=[[1001, 1]],
            ts_event=1_000_000,
            ts_init=1_000_000,
        )

        self.data_engine.process(snapshot)
        events = self.clock.advance_time(1_000_000_000)
        events[0].handle()

        # Act
        self.data_engine.process(snapshot)

        # Assert
        cached_book = self.cache.order_book(ETHUSDT_BINANCE.id)
        assert isinstance(cached_book, L2OrderBook)
        assert cached_book.instrument_id == ETHUSDT_BINANCE.id
        assert handler1[0] == cached_book
        assert handler2[0] == cached_book

    def test_order_book_delta_creates_book(self):
        # Arrange
        self.data_engine.register_client(self.betfair)
        self.betfair.start()
        self.data_engine.process(BETFAIR_INSTRUMENT)  # <-- add necessary instrument for test

        subscribe = Subscribe(
            client_id=ClientId(BETFAIR.value),
            venue=BETFAIR,
            data_type=DataType(
                OrderBookDelta,
                metadata={
                    "instrument_id": BETFAIR_INSTRUMENT.id,
                    "book_type": 2,
                    "depth": 25,
                    "interval_ms": 1000,
                },
            ),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        deltas = OrderBookDeltas(
            instrument_id=BETFAIR_INSTRUMENT.id,
            deltas=[TestDataStubs.order_book_delta(instrument_id=BETFAIR_INSTRUMENT.id)],
            ts_event=1_000_000,
            ts_init=1_000_000,
        )

        # Act
        self.data_engine.process(deltas)

        # Assert
        cached_book = self.cache.order_book(BETFAIR_INSTRUMENT.id)
        assert isinstance(cached_book, L2OrderBook)
        assert cached_book.instrument_id == BETFAIR_INSTRUMENT.id
        assert cached_book.best_bid_price() == 100

    def test_execute_subscribe_ticker(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        handler = []
        self.msgbus.subscribe(topic="data.quotes.BINANCE.ETH/USD", handler=handler.append)

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Ticker, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        # Assert
        assert self.data_engine.subscribed_tickers() == [ETHUSDT_BINANCE.id]

    def test_execute_unsubscribe_ticker(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        handler = []
        self.msgbus.subscribe(topic="data.quotes.BINANCE.ETH/USD", handler=handler.append)

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Ticker, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        unsubscribe = Unsubscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Ticker, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.subscribed_tickers() == []

    def test_execute_subscribe_quote_ticks(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        handler = []
        self.msgbus.subscribe(topic="data.quotes.BINANCE.ETH/USD", handler=handler.append)

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(QuoteTick, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        assert self.data_engine.subscribed_quote_ticks() == [ETHUSDT_BINANCE.id]

    def test_execute_unsubscribe_quote_ticks(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        handler = []
        self.msgbus.subscribe(topic="data.quotes.BINANCE.ETH/USD", handler=handler.append)

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(QuoteTick, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        unsubscribe = Unsubscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(QuoteTick, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.subscribed_quote_ticks() == []

    def test_process_quote_tick_when_subscriber_then_sends_to_registered_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        handler = []
        self.msgbus.subscribe(topic="data.quotes.BINANCE.ETHUSDT", handler=handler.append)

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(QuoteTick, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        tick = QuoteTick(
            instrument_id=ETHUSDT_BINANCE.id,
            bid=Price.from_str("100.003"),
            ask=Price.from_str("100.003"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        assert handler == [tick]

    def test_process_quote_tick_when_subscribers_then_sends_to_registered_handlers(
        self,
    ):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        handler1 = []
        handler2 = []

        self.msgbus.subscribe(topic="data.quotes.BINANCE.ETHUSDT", handler=handler1.append)
        self.msgbus.subscribe(topic="data.quotes.BINANCE.ETHUSDT", handler=handler2.append)

        subscribe1 = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(QuoteTick, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        subscribe2 = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(QuoteTick, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe1)
        self.data_engine.execute(subscribe2)

        tick = QuoteTick(
            instrument_id=ETHUSDT_BINANCE.id,
            bid=Price.from_str("100.003"),
            ask=Price.from_str("100.003"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        assert handler1 == [tick]
        assert handler2 == [tick]

    def test_subscribe_trade_tick_then_subscribes(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(TradeTick, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        assert self.data_engine.subscribed_trade_ticks() == [ETHUSDT_BINANCE.id]

    def test_unsubscribe_trade_tick_then_unsubscribes(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        handler = []
        self.msgbus.subscribe(topic="data.trades.BINANCE.ETH/USD", handler=handler.append)

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(TradeTick, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        unsubscribe = Unsubscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(TradeTick, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.subscribed_trade_ticks() == []

    def test_process_trade_tick_when_subscriber_then_sends_to_registered_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        handler = []
        self.msgbus.subscribe(topic="data.trades.BINANCE.ETHUSDT", handler=handler.append)

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(TradeTick, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        tick = TradeTick(
            instrument_id=ETHUSDT_BINANCE.id,
            price=Price.from_str("1050.00000"),
            size=Quantity.from_int(100),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456789"),
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        assert handler == [tick]

    def test_process_trade_tick_when_subscribers_then_sends_to_registered_handlers(
        self,
    ):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        handler1 = []
        handler2 = []
        self.msgbus.subscribe(topic="data.trades.BINANCE.ETHUSDT", handler=handler1.append)
        self.msgbus.subscribe(topic="data.trades.BINANCE.ETHUSDT", handler=handler2.append)

        subscribe1 = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(TradeTick, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        subscribe2 = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(TradeTick, metadata={"instrument_id": ETHUSDT_BINANCE.id}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe1)
        self.data_engine.execute(subscribe2)

        tick = TradeTick(
            instrument_id=ETHUSDT_BINANCE.id,
            price=Price.from_str("1050.00000"),
            size=Quantity.from_int(100),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456789"),
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        assert handler1 == [tick]
        assert handler2 == [tick]

    def test_subscribe_bar_type_then_subscribes(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(ETHUSDT_BINANCE.id, bar_spec)

        handler = []
        self.msgbus.subscribe(topic=f"data.bars.{bar_type}", handler=handler.append)

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Bar, metadata={"bar_type": bar_type}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        assert self.data_engine.command_count == 1
        assert self.data_engine.subscribed_bars() == [bar_type]

    def test_unsubscribe_bar_type_then_unsubscribes(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(ETHUSDT_BINANCE.id, bar_spec)

        handler = []
        self.msgbus.subscribe(topic=f"data.bars.{bar_type}", handler=handler.append)

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Bar, metadata={"bar_type": bar_type}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        self.msgbus.unsubscribe(topic=f"data.bars.{bar_type}", handler=handler.append)
        unsubscribe = Unsubscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Bar, metadata={"bar_type": bar_type}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.command_count == 2
        assert self.data_engine.subscribed_bars() == []

    def test_process_bar_when_subscriber_then_sends_to_registered_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(ETHUSDT_BINANCE.id, bar_spec)

        handler = []
        self.msgbus.subscribe(topic=f"data.bars.{bar_type}", handler=handler.append)

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Bar, metadata={"bar_type": bar_type}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
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
        assert handler == [bar]

    def test_process_bar_when_subscribers_then_sends_to_registered_handlers(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(ETHUSDT_BINANCE.id, bar_spec)

        handler1 = []
        handler2 = []
        self.msgbus.subscribe(topic=f"data.bars.{bar_type}", handler=handler1.append)
        self.msgbus.subscribe(topic=f"data.bars.{bar_type}", handler=handler2.append)

        subscribe1 = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Bar, metadata={"bar_type": bar_type}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        subscribe2 = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Bar, metadata={"bar_type": bar_type}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
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
        assert handler1 == [bar]
        assert handler2 == [bar]

    def test_process_bar_when_with_older_timestamp_does_not_cache_or_publish(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(ETHUSDT_BINANCE.id, bar_spec)

        handler = []
        self.msgbus.subscribe(topic=f"data.bars.{bar_type}", handler=handler.append)

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Bar, metadata={"bar_type": bar_type}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        bar1 = Bar(
            bar_type,
            Price.from_str("1051.00000"),
            Price.from_str("1055.00000"),
            Price.from_str("1050.00000"),
            Price.from_str("1052.00000"),
            Quantity.from_int(100),
            1,
            1,
        )

        bar2 = Bar(
            bar_type,
            Price.from_str("1051.00000"),
            Price.from_str("1055.00000"),
            Price.from_str("1050.00000"),
            Price.from_str("1051.00000"),
            Quantity.from_int(100),
            0,
            1,
        )

        bar3 = Bar(
            bar_type,
            Price.from_str("1051.00000"),
            Price.from_str("1055.00000"),
            Price.from_str("1050.00000"),
            Price.from_str("1050.50000"),
            Quantity.from_int(100),
            0,
            0,
        )

        bar4 = Bar(
            bar_type,
            Price.from_str("1051.00000"),
            Price.from_str("1055.00000"),
            Price.from_str("1049.00000"),
            Price.from_str("1049.50000"),
            Quantity.from_int(100),
            2,
            0,
        )

        # Act
        self.data_engine.process(bar1)
        self.data_engine.process(bar2)
        self.data_engine.process(bar3)
        self.data_engine.process(bar4)

        # Assert
        assert handler == [bar1]
        assert self.cache.bar(bar_type) == bar1

    def test_process_bar_when_revision_is_not_of_last_bar_does_not_cache_or_publish(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(ETHUSDT_BINANCE.id, bar_spec)

        handler = []
        self.msgbus.subscribe(topic=f"data.bars.{bar_type}", handler=handler.append)

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Bar, metadata={"bar_type": bar_type}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        bar1 = Bar(
            bar_type,
            Price.from_str("1051.00000"),
            Price.from_str("1055.00000"),
            Price.from_str("1050.00000"),
            Price.from_str("1052.00000"),
            Quantity.from_int(100),
            1,
            1,
        )

        bar2 = Bar(
            bar_type,
            Price.from_str("1051.00000"),
            Price.from_str("1053.00000"),
            Price.from_str("1050.00000"),
            Price.from_str("1051.00000"),
            Quantity.from_int(100),
            1,
            3,
            is_revision=True,
        )

        bar3 = Bar(
            bar_type,
            Price.from_str("1051.00000"),
            Price.from_str("1052.00000"),
            Price.from_str("1050.00000"),
            Price.from_str("1051.00000"),
            Quantity.from_int(100),
            1,
            2,
            is_revision=True,
        )

        # Act
        self.data_engine.process(bar1)
        self.data_engine.process(bar2)
        self.data_engine.process(bar3)

        # Assert
        assert handler == [bar1, bar2]
        assert self.cache.bar(bar_type) == bar2

    def test_process_bar_when_revision_is_set_but_is_actually_new_bar(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(ETHUSDT_BINANCE.id, bar_spec)

        handler = []
        self.msgbus.subscribe(topic=f"data.bars.{bar_type}", handler=handler.append)

        subscribe = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(Bar, metadata={"bar_type": bar_type}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        bar1 = Bar(
            bar_type,
            Price.from_str("1051.00000"),
            Price.from_str("1055.00000"),
            Price.from_str("1050.00000"),
            Price.from_str("1052.00000"),
            Quantity.from_int(100),
            1,
            1,
        )

        bar2 = Bar(
            bar_type,
            Price.from_str("1051.00000"),
            Price.from_str("1053.00000"),
            Price.from_str("1050.00000"),
            Price.from_str("1051.00000"),
            Quantity.from_int(100),
            2,
            2,
            is_revision=True,  # <- Important
        )

        # Act
        self.data_engine.process(bar1)
        self.data_engine.process(bar2)

        # Assert
        assert handler == [bar1, bar2]
        assert self.cache.bar(bar_type) == bar2

    def test_request_instrument_reaches_client(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        handler = []
        request = DataRequest(
            client_id=None,
            venue=BINANCE,
            data_type=DataType(
                Instrument,
                metadata={  # str data type is invalid
                    "instrument_id": ETHUSDT_BINANCE.id,
                },
            ),
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert self.data_engine.request_count == 1
        assert len(handler) == 1
        assert handler[0].data == ETHUSDT_BINANCE

    def test_request_instruments_reaches_client(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        handler = []
        request = DataRequest(
            client_id=None,
            venue=BINANCE,
            data_type=DataType(
                Instrument,
                metadata={  # str data type is invalid
                    "venue": BINANCE,
                },
            ),
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert self.data_engine.request_count == 1
        assert len(handler) == 1
        assert handler[0].data == [BTCUSDT_BINANCE, ETHUSDT_BINANCE]

    def test_request_instrument_when_catalog_registered(self):
        # Arrange
        catalog = data_catalog_setup(protocol="file")

        idealpro = Venue("IDEALPRO")
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=idealpro)
        write_objects(catalog=catalog, chunk=[instrument])

        self.data_engine.register_catalog(catalog)

        # Act
        handler = []
        request = DataRequest(
            client_id=None,
            venue=idealpro,
            data_type=DataType(Instrument, metadata={"instrument_id": instrument.id}),
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert self.data_engine.request_count == 1
        assert len(handler) == 1
        assert len(handler[0].data) == 1

    def test_request_instruments_for_venue_when_catalog_registered(self):
        # Arrange
        catalog = data_catalog_setup(protocol="file")

        idealpro = Venue("IDEALPRO")
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=idealpro)
        write_objects(catalog=catalog, chunk=[instrument])

        self.data_engine.register_catalog(catalog)

        # Act
        handler = []
        request = DataRequest(
            client_id=None,
            venue=idealpro,
            data_type=DataType(Instrument, metadata={}),
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert self.data_engine.request_count == 1
        assert len(handler) == 1
        assert len(handler[0].data) == 1

    # TODO: Implement with new Rust datafusion backend"
    # def test_request_quote_ticks_when_catalog_registered_using_rust(self) -> None:
    #     # Arrange
    #     catalog = data_catalog_setup(protocol="file")
    #     self.clock.set_time(to_time_ns=1638058200000000000)  # <- Set to end of data
    #
    #     parquet_data_path = os.path.join(TEST_DATA_DIR, "quote_tick_data.parquet")
    #     assert os.path.exists(parquet_data_path)
    #     reader = ParquetReader(
    #         parquet_data_path,
    #         100,
    #         ParquetType.QuoteTick,
    #         ParquetReaderType.File,
    #     )
    #
    #     mapped_chunk = map(QuoteTick.list_from_capsule, reader)
    #     ticks = list(itertools.chain(*mapped_chunk))
    #
    #     min_timestamp = str(ticks[0].ts_init).rjust(19, "0")
    #     max_timestamp = str(ticks[-1].ts_init).rjust(19, "0")
    #
    #     sim_venue = Venue("SIM")
    #
    #     # Reset reader
    #     reader = ParquetReader(
    #         parquet_data_path,
    #         100,
    #         ParquetType.QuoteTick,
    #         ParquetReaderType.File,
    #     )
    #
    #     metadata = {
    #         "instrument_id": f"EUR/USD.{sim_venue}",
    #         "price_precision": "5",
    #         "size_precision": "0",
    #     }
    #     writer = ParquetWriter(
    #         ParquetType.QuoteTick,
    #         metadata,
    #     )
    #
    #     file_path = os.path.join(
    #         catalog.path,
    #         "data",
    #         "quote_tick.parquet",
    #         f"instrument_id=EUR-USD.{sim_venue}",
    #         f"{min_timestamp}-{max_timestamp}-0.parquet",
    #     )
    #
    #     os.makedirs(os.path.dirname(file_path), exist_ok=True)
    #     with open(file_path, "wb") as f:
    #         for chunk in reader:
    #             writer.write(chunk)
    #         data: bytes = writer.flush_bytes()
    #         f.write(data)
    #
    #     self.data_engine.register_catalog(catalog, use_rust=True)
    #
    #     # Act
    #     handler: list[DataResponse] = []
    #     request = DataRequest(
    #         client_id=None,
    #         venue=sim_venue,
    #         data_type=DataType(
    #             QuoteTick,
    #             metadata={
    #                 "instrument_id": InstrumentId(Symbol("EUR/USD"), sim_venue),
    #             },
    #         ),
    #         callback=handler.append,
    #         request_id=UUID4(),
    #         ts_init=self.clock.timestamp_ns(),
    #     )
    #
    #     # Act
    #     self.msgbus.request(endpoint="DataEngine.request", request=request)
    #
    #     # Assert
    #     assert self.data_engine.request_count == 1
    #     assert len(handler) == 1
    #     assert len(handler[0].data) == 9500
    #     assert isinstance(handler[0].data, list)
    #     assert isinstance(handler[0].data[0], QuoteTick)

    # def test_request_trade_ticks_when_catalog_registered_using_rust(self) -> None:
    #     # Arrange
    #     catalog = data_catalog_setup(protocol="file")
    #     self.clock.set_time(to_time_ns=1638058200000000000)  # <- Set to end of data
    #
    #     parquet_data_path = os.path.join(TEST_DATA_DIR, "trade_tick_data.parquet")
    #     assert os.path.exists(parquet_data_path)
    #     reader = ParquetReader(
    #         parquet_data_path,
    #         100,
    #         ParquetType.TradeTick,
    #         ParquetReaderType.File,
    #     )
    #
    #     mapped_chunk = map(TradeTick.list_from_capsule, reader)
    #     trades = list(itertools.chain(*mapped_chunk))
    #
    #     min_timestamp = str(trades[0].ts_init).rjust(19, "0")
    #     max_timestamp = str(trades[-1].ts_init).rjust(19, "0")
    #
    #     sim_venue = Venue("SIM")
    #
    #     # Reset reader
    #     reader = ParquetReader(
    #         parquet_data_path,
    #         100,
    #         ParquetType.TradeTick,
    #         ParquetReaderType.File,
    #     )
    #
    #     metadata = {
    #         "instrument_id": f"EUR/USD.{sim_venue}",
    #         "price_precision": "5",
    #         "size_precision": "0",
    #     }
    #     writer = ParquetWriter(
    #         ParquetType.TradeTick,
    #         metadata,
    #     )
    #
    #     file_path = os.path.join(
    #         catalog.path,
    #         "data",
    #         "trade_tick.parquet",
    #         f"instrument_id=EUR-USD.{sim_venue}",
    #         f"{min_timestamp}-{max_timestamp}-0.parquet",
    #     )
    #
    #     os.makedirs(os.path.dirname(file_path), exist_ok=True)
    #     with open(file_path, "wb") as f:
    #         for chunk in reader:
    #             writer.write(chunk)
    #         data: bytes = writer.flush_bytes()
    #         f.write(data)
    #
    #     self.data_engine.register_catalog(catalog, use_rust=True)
    #
    #     # Act
    #     handler: list[DataResponse] = []
    #     request1 = DataRequest(
    #         client_id=None,
    #         venue=sim_venue,
    #         data_type=DataType(
    #             TradeTick,
    #             metadata={
    #                 "instrument_id": InstrumentId(Symbol("EUR/USD"), sim_venue),
    #             },
    #         ),
    #         callback=handler.append,
    #         request_id=UUID4(),
    #         ts_init=self.clock.timestamp_ns(),
    #     )
    #     request2 = DataRequest(
    #         client_id=None,
    #         venue=sim_venue,
    #         data_type=DataType(
    #             TradeTick,
    #             metadata={
    #                 "instrument_id": InstrumentId(Symbol("EUR/USD"), sim_venue),
    #                 "start": UNIX_EPOCH,
    #                 "end": pd.Timestamp(sys.maxsize, tz="UTC"),
    #             },
    #         ),
    #         callback=handler.append,
    #         request_id=UUID4(),
    #         ts_init=self.clock.timestamp_ns(),
    #     )
    #
    #     # Act
    #     self.msgbus.request(endpoint="DataEngine.request", request=request1)
    #     self.msgbus.request(endpoint="DataEngine.request", request=request2)
    #
    #     # Assert
    #     assert self.data_engine.request_count == 2
    #     assert len(handler) == 2
    #     assert len(handler[0].data) == 100
    #     assert len(handler[1].data) == 100
    #     assert isinstance(handler[0].data, list)
    #     assert isinstance(handler[0].data[0], TradeTick)

    def test_request_bars_when_catalog_registered(self):
        # Arrange
        catalog = data_catalog_setup(protocol="file")
        self.clock.set_time(to_time_ns=1638058200000000000)  # <- Set to end of data

        bar_type = TestDataStubs.bartype_adabtc_binance_1min_last()
        instrument = TestInstrumentProvider.adabtc_binance()
        wrangler = BarDataWrangler(bar_type, instrument)

        def parser(data):
            data["timestamp"] = data["timestamp"].astype("datetime64[ms]")
            bars = wrangler.process(data.set_index("timestamp"))
            return bars

        binance_spot_header = [
            "timestamp",
            "open",
            "high",
            "low",
            "close",
            "volume",
            "ts_close",
            "quote_volume",
            "n_trades",
            "taker_buy_base_volume",
            "taker_buy_quote_volume",
            "ignore",
        ]
        reader = CSVReader(block_parser=parser, header=binance_spot_header)

        _ = process_files(
            glob_path=f"{TEST_DATA_DIR}/ADABTC-1m-2021-11-*.csv",
            reader=reader,
            catalog=catalog,
        )

        self.data_engine.register_catalog(catalog)

        # Act
        handler = []
        request = DataRequest(
            client_id=None,
            venue=BINANCE,
            data_type=DataType(
                Bar,
                metadata={
                    "bar_type": BarType(
                        InstrumentId(Symbol("ADABTC"), BINANCE),
                        BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST),
                    ),
                    "start": UNIX_EPOCH,
                    "end": pd.Timestamp(sys.maxsize, tz="UTC"),
                },
            ),
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert self.data_engine.request_count == 1
        assert len(handler) == 1
        assert len(handler[0].data) == 21
        assert handler[0].data[0].ts_init == 1637971200000000000
        assert handler[0].data[-1].ts_init == 1638058200000000000
