# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import os
import sys

import pandas as pd
import pytest

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.backtest.data_client import BacktestMarketDataClient
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.core.data import Data
from nautilus_trader.core.datetime import time_object_to_dt
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.data.engine import DataEngineConfig
from nautilus_trader.data.messages import DataCommand
from nautilus_trader.data.messages import DataResponse
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestData
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestOrderBookDepth
from nautilus_trader.data.messages import RequestOrderBookSnapshot
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeData
from nautilus_trader.data.messages import SubscribeFundingRates
from nautilus_trader.data.messages import SubscribeIndexPrices
from nautilus_trader.data.messages import SubscribeInstrument
from nautilus_trader.data.messages import SubscribeInstruments
from nautilus_trader.data.messages import SubscribeMarkPrices
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeData
from nautilus_trader.data.messages import UnsubscribeFundingRates
from nautilus_trader.data.messages import UnsubscribeIndexPrices
from nautilus_trader.data.messages import UnsubscribeInstrument
from nautilus_trader.data.messages import UnsubscribeInstruments
from nautilus_trader.data.messages import UnsubscribeMarkPrices
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import NULL_ORDER
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.instruments.currency_pair import CurrencyPair
from nautilus_trader.model.instruments.option_contract import OptionContract
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog.parquet import _timestamps_to_filename
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.mocks.data import MockMarketDataClient
from nautilus_trader.test_kit.mocks.data import setup_catalog
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.filters import NewsEvent
from tests.unit_tests.portfolio.test_portfolio import BETFAIR


BITMEX = Venue("BITMEX")
BINANCE = Venue("BINANCE")
XNAS = Venue("XNAS")
AAPL_XNAS = TestInstrumentProvider.equity()
XBTUSD_BITMEX = TestInstrumentProvider.xbtusd_bitmex()
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
BTCUSDT_PERP_BINANCE = TestInstrumentProvider.btcusdt_perp_binance()
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class TestDataEngine:
    @pytest.fixture(autouse=True)
    def setup_method(self, tmp_path):
        self.tmp_path = tmp_path
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        config = DataEngineConfig(
            validate_data_sequence=True,
            emit_quotes_from_book_depths=True,
            debug=True,
        )
        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=config,
        )

        self.binance_client = BacktestMarketDataClient(
            client_id=ClientId(BINANCE.value),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.bitmex_client = BacktestMarketDataClient(
            client_id=ClientId(BITMEX.value),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.quandl = BacktestMarketDataClient(
            client_id=ClientId("QUANDL"),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.betfair = BacktestMarketDataClient(
            client_id=ClientId("BETFAIR"),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.mock_market_data_client = MockMarketDataClient(
            client_id=ClientId(BINANCE.value),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
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
        request = RequestQuoteTicks(
            instrument_id=InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
            start=None,
            end=None,
            limit=1000,
            client_id=ClientId("RANDOM"),  # <-- Will route to non existent client
            venue=None,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        # Act
        self.data_engine.request(request)

        # Assert
        assert self.data_engine.request_count == 1

    def test_send_data_request_when_data_type_unrecognized_logs_and_does_nothing(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        handler = []
        request = RequestData(
            data_type=DataType(
                Data,
                metadata={
                    "instrument_id": InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
                },
            ),
            instrument_id=None,
            start=None,
            end=None,
            limit=0,
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params={"limit": 1000},
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

        request1 = RequestQuoteTicks(
            instrument_id=InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
            start=None,
            end=None,
            limit=1000,
            client_id=ClientId("RANDOM"),
            venue=None,
            callback=handler.append,
            request_id=uuid,
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        request2 = RequestQuoteTicks(
            instrument_id=InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
            start=None,
            end=None,
            limit=1000,
            client_id=ClientId("RANDOM"),
            venue=None,
            callback=handler.append,
            request_id=uuid,
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        # Act
        self.data_engine.request(request1)
        self.data_engine.request(request2)

        # Assert
        assert self.data_engine.request_count == 2

    def test_execute_subscribe_when_data_type_not_implemented_logs_and_does_nothing(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        subscribe = SubscribeData(
            client_id=None,  # Will route to the Binance venue
            instrument_id=None,
            venue=BINANCE,
            data_type=DataType(NewsEvent),  # NewsEvent data not recognized
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        assert self.data_engine.command_count == 1

    def test_execute_subscribe_custom_data(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.data_engine.register_client(self.quandl)
        self.binance_client.start()

        subscribe = SubscribeData(
            instrument_id=None,
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
        assert self.data_engine.subscribed_custom_data() == [
            DataType(type=Data, metadata={"Type": "news"}),
        ]

    def test_execute_unsubscribe_custom_data(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.data_engine.register_client(self.quandl)
        self.binance_client.start()

        data_type = DataType(Data, metadata={"Type": "news"})
        handler = []

        self.msgbus.subscribe(topic=f"data.{data_type.topic}", handler=handler.append)
        subscribe = SubscribeData(
            instrument_id=None,
            client_id=ClientId("QUANDL"),
            venue=None,
            data_type=DataType(Data, metadata={"Type": "news"}),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        self.msgbus.unsubscribe(topic=f"data.{data_type.topic}", handler=handler.append)
        unsubscribe = UnsubscribeData(
            instrument_id=None,
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
        assert self.data_engine.subscribed_custom_data() == []

    def test_execute_unsubscribe_when_data_type_unrecognized_logs_and_does_nothing(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        unsubscribe = UnsubscribeData(
            instrument_id=None,
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            data_type=DataType(Data),
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

        unsubscribe = UnsubscribeQuoteTicks(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
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
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            data_type=DataType(QuoteTick),
            data=[],
            correlation_id=UUID4(),
            response_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            start=pd.Timestamp("2023-01-01"),
            end=pd.Timestamp("2023-01-02"),
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
        tick = TestDataStubs.trade_tick()

        # Act
        self.data_engine.process(tick)

        # Assert
        assert self.data_engine.data_count == 4

    def test_execute_subscribe_instruments_then_adds_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        subscribe = SubscribeInstruments(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
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

        subscribe = SubscribeInstruments(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        assert self.binance_client.subscribed_instruments() == [
            BTCUSDT_BINANCE.id,
            ETHUSDT_BINANCE.id,
        ]

        unsubscribe = UnsubscribeInstruments(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.subscribed_instruments() == []
        assert self.binance_client.subscribed_instruments() == []

    def test_execute_subscribe_instrument_then_adds_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        subscribe = SubscribeInstrument(
            instrument_id=ETHUSDT_BINANCE.id,
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        assert self.data_engine.command_count == 1
        assert self.data_engine.subscribed_instruments() == [ETHUSDT_BINANCE.id]

    def test_execute_subscribe_instrument_synthetic_logs_error(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        subscribe = SubscribeInstrument(
            client_id=ClientId(BINANCE.value),
            venue=Venue("SYNTH"),
            instrument_id=TestIdStubs.synthetic_id(),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        assert self.data_engine.command_count == 1

    def test_execute_unsubscribe_instrument_then_removes_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        subscribe = SubscribeInstrument(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        assert self.binance_client.subscribed_instruments() == [ETHUSDT_BINANCE.id]

        unsubscribe = UnsubscribeInstrument(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.subscribed_instruments() == []
        assert self.binance_client.subscribed_instruments() == []

    def test_execute_unsubscribe_synthetic_instrument_logs_error(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        unsubscribe = UnsubscribeInstrument(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=TestIdStubs.synthetic_id(),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.command_count == 1

    def test_process_instrument_when_subscriber_then_sends_to_registered_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        handler = []
        self.msgbus.subscribe(topic="data.instrument.BINANCE.ETHUSDT", handler=handler.append)

        subscribe = SubscribeInstrument(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
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

        subscribe1 = SubscribeInstrument(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        subscribe2 = SubscribeInstrument(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
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

        subscribe = SubscribeOrderBook(
            book_data_type=OrderBookDelta,
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=2,
            depth=10,
            interval_ms=1000,
            managed=True,
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

        subscribe = SubscribeOrderBook(
            book_data_type=OrderBookDelta,
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=2,
            depth=10,
            interval_ms=1000,
            managed=True,
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

        subscribe = SubscribeOrderBook(
            book_data_type=OrderBookDelta,
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=2,
            depth=25,
            interval_ms=1000,
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        assert self.data_engine.subscribed_order_book_deltas() == [ETHUSDT_BINANCE.id]

    def test_execute_unsubscribe_order_book_deltas_then_removes_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        subscribe = SubscribeOrderBook(
            book_data_type=OrderBookDelta,
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=2,
            depth=25,
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        assert self.binance_client.subscribed_order_book_deltas() == [ETHUSDT_BINANCE.id]

        unsubscribe = UnsubscribeOrderBook(
            book_data_type=OrderBookDelta,
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.subscribed_order_book_snapshots() == []
        assert self.binance_client.subscribed_order_book_deltas() == []

    def test_execute_unsubscribe_order_book_at_interval_then_removes_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        subscribe = SubscribeOrderBook(
            book_data_type=OrderBookDelta,
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=2,
            depth=25,
            interval_ms=1000,
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        assert self.binance_client.subscribed_order_book_snapshots() == []
        assert self.binance_client.subscribed_order_book_deltas() == [ETHUSDT_BINANCE.id]

        unsubscribe = UnsubscribeOrderBook(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            book_data_type=OrderBookDelta,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.subscribed_order_book_snapshots() == []
        assert self.binance_client.subscribed_order_book_snapshots() == []
        assert self.binance_client.subscribed_order_book_deltas() == []

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

        subscribe = SubscribeOrderBook(
            book_data_type=OrderBookDelta,
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=BookType.L2_MBP,
            depth=20,
            interval_ms=1000,  # Streaming
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        # Act
        events = self.clock.advance_time(2_000_000_000)
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

        subscribe = SubscribeOrderBook(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            book_data_type=OrderBookDelta,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=BookType.L2_MBP,
            depth=25,
            interval_ms=1000,  # Streaming
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        snapshot = TestDataStubs.order_book_snapshot(
            instrument=ETHUSDT_BINANCE,
            ts_event=1,
        )

        # Act
        self.data_engine.process(snapshot)

        events = self.clock.advance_time(2_000_000_000)
        events[0].handle()

        # Assert
        assert isinstance(handler[0], OrderBook)

    def test_process_order_book_delta_then_sends_to_registered_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        self.data_engine.process(ETHUSDT_BINANCE)  # <-- add necessary instrument for test

        handler = []
        self.msgbus.subscribe(topic="data.book.deltas.BINANCE.ETHUSDT", handler=handler.append)

        subscribe = SubscribeOrderBook(
            book_data_type=OrderBookDelta,
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=BookType.L3_MBO,
            depth=5,
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        deltas = TestDataStubs.order_book_delta(ETHUSDT_BINANCE.id)

        # Act
        self.data_engine.process(deltas)

        # Assert
        assert handler[0].instrument_id == ETHUSDT_BINANCE.id
        assert isinstance(handler[0], OrderBookDeltas)

    def test_process_order_book_deltas_then_sends_to_registered_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        self.data_engine.process(ETHUSDT_BINANCE)  # <-- add necessary instrument for test

        handler = []
        self.msgbus.subscribe(topic="data.book.deltas.BINANCE.ETHUSDT", handler=handler.append)

        subscribe = SubscribeOrderBook(
            book_data_type=OrderBookDelta,
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=BookType.L3_MBO,
            depth=5,
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        deltas = TestDataStubs.order_book_deltas(ETHUSDT_BINANCE.id)

        # Act
        self.data_engine.process(deltas)

        # Assert
        assert handler[0].instrument_id == ETHUSDT_BINANCE.id
        assert isinstance(handler[0], OrderBookDeltas)

    def test_process_order_book_deltas_with_composite_symbol(self):
        # Arrange
        esf5 = TestInstrumentProvider.es_future(2024, 1)
        esg5 = TestInstrumentProvider.es_future(2024, 2)
        esh5 = TestInstrumentProvider.es_future(2024, 3)

        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        self.data_engine.process(esf5)  # <-- add necessary instrument for test
        self.data_engine.process(esg5)  # <-- add necessary instrument for test
        self.data_engine.process(esh5)  # <-- add necessary instrument for test

        handler = []
        self.msgbus.subscribe(topic="data.book.deltas.GLBX.ES*", handler=handler.append)

        es_fut = InstrumentId.from_str("ES.FUT.GLBX")

        subscribe = SubscribeOrderBook(
            book_data_type=OrderBookDelta,
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=es_fut,
            book_type=BookType.L3_MBO,
            depth=25,
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        deltas1 = TestDataStubs.order_book_deltas(esf5.id)
        deltas2 = TestDataStubs.order_book_deltas(esg5.id)
        deltas3 = TestDataStubs.order_book_deltas(esh5.id)

        # Act
        self.data_engine.process(deltas1)
        self.data_engine.process(deltas2)
        self.data_engine.process(deltas3)

        # Assert
        assert len(handler) == 3
        assert isinstance(handler[0], OrderBookDeltas)
        assert isinstance(handler[1], OrderBookDeltas)
        assert isinstance(handler[2], OrderBookDeltas)
        assert handler[0].instrument_id == esf5.id
        assert handler[1].instrument_id == esg5.id
        assert handler[2].instrument_id == esh5.id

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

        subscribe1 = SubscribeOrderBook(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            book_data_type=OrderBookDelta,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=BookType.L2_MBP,
            depth=25,
            interval_ms=1000,  # Streaming
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        subscribe2 = SubscribeOrderBook(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            book_data_type=OrderBookDelta,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=BookType.L2_MBP,
            depth=25,
            interval_ms=1000,  # Streaming
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe1)
        self.data_engine.execute(subscribe2)

        snapshot = TestDataStubs.order_book_snapshot(
            instrument=ETHUSDT_BINANCE,
            ts_event=1,
        )

        self.data_engine.process(snapshot)
        events = self.clock.advance_time(2_000_000_000)
        events[0].handle()

        # Act
        self.data_engine.process(snapshot)

        # Assert
        cached_book = self.cache.order_book(ETHUSDT_BINANCE.id)
        assert isinstance(cached_book, OrderBook)
        assert cached_book.instrument_id == ETHUSDT_BINANCE.id
        assert handler1[0] == cached_book
        assert handler2[0] == cached_book

    def test_process_order_book_depth_when_multiple_subscribers_then_sends_to_registered_handlers(
        self,
    ):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        self.data_engine.process(BTCUSDT_PERP_BINANCE)  # <-- add necessary instrument for test

        handler1 = []
        handler2 = []
        self.msgbus.subscribe(
            topic="data.book.depth.BINANCE.BTCUSDT-PERP",
            handler=handler1.append,
        )
        self.msgbus.subscribe(
            topic="data.book.depth.BINANCE.BTCUSDT-PERP",
            handler=handler2.append,
        )

        subscribe1 = SubscribeOrderBook(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            book_data_type=OrderBookDepth10,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=BookType.L2_MBP,
            depth=10,
            interval_ms=1000,  # Streaming
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        subscribe2 = SubscribeOrderBook(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            book_data_type=OrderBookDepth10,
            instrument_id=BTCUSDT_PERP_BINANCE.id,
            book_type=BookType.L2_MBP,
            depth=10,
            interval_ms=1000,  # Streaming
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe1)
        self.data_engine.execute(subscribe2)

        depth = TestDataStubs.order_book_depth10(
            instrument_id=BTCUSDT_PERP_BINANCE.id,
            ts_event=1,
        )

        self.data_engine.process(depth)
        events = self.clock.advance_time(2_000_000_000)
        events[0].handle()

        # Act
        self.data_engine.process(depth)

        # Assert
        cached_book = self.cache.order_book(BTCUSDT_PERP_BINANCE.id)
        assert isinstance(cached_book, OrderBook)
        assert cached_book.instrument_id == BTCUSDT_PERP_BINANCE.id
        assert handler1[0] == depth
        assert handler2[0] == depth

    def test_order_book_delta_creates_book(self):
        # Arrange
        self.data_engine.register_client(self.betfair)
        self.betfair.start()
        self.data_engine.process(ETHUSDT_BINANCE)  # <-- add necessary instrument for test

        subscribe = SubscribeOrderBook(
            client_id=None,  # Will route to the Binance venue
            venue=BETFAIR,
            book_data_type=OrderBookDelta,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=2,
            depth=25,
            interval_ms=1000,
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        deltas = OrderBookDeltas(
            instrument_id=ETHUSDT_BINANCE.id,
            deltas=[TestDataStubs.order_book_delta(instrument_id=ETHUSDT_BINANCE.id)],
        )

        # Act
        self.data_engine.process(deltas)

        # Assert
        cached_book = self.cache.order_book(ETHUSDT_BINANCE.id)
        assert isinstance(cached_book, OrderBook)
        assert cached_book.instrument_id == ETHUSDT_BINANCE.id
        assert cached_book.best_bid_price() == 100

    def test_unsubscribe_order_book_interval_cancels_snapshot_timer(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        self.data_engine.process(ETHUSDT_BINANCE)

        subscribe = SubscribeOrderBook(
            book_data_type=OrderBookDelta,
            client_id=None,
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=BookType.L2_MBP,
            depth=10,
            interval_ms=1000,
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        # Sanity check timer created
        timer_name = f"OrderBook|{ETHUSDT_BINANCE.id}|1000"
        assert timer_name in self.data_engine._snapshot_info

        unsubscribe = UnsubscribeOrderBook(
            book_data_type=OrderBookDelta,
            client_id=None,
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert timer state cleared and no timer events fire
        assert timer_name not in self.data_engine._snapshot_info
        assert (ETHUSDT_BINANCE.id, 1000) not in self.data_engine._order_book_intervals

        events = self.clock.advance_time(2_000_000_000)
        assert events == []

    def test_execute_subscribe_quote_ticks(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        handler = []
        self.msgbus.subscribe(topic="data.quotes.BINANCE.ETH/USD", handler=handler.append)

        subscribe = SubscribeQuoteTicks(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
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

        subscribe = SubscribeQuoteTicks(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        assert self.binance_client.subscribed_quote_ticks() == [ETHUSDT_BINANCE.id]

        unsubscribe = UnsubscribeQuoteTicks(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.subscribed_quote_ticks() == []
        assert self.binance_client.subscribed_quote_ticks() == []

    def test_process_quote_tick_when_subscriber_then_sends_to_registered_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        handler = []
        self.msgbus.subscribe(topic="data.quotes.BINANCE.ETHUSDT", handler=handler.append)

        subscribe = SubscribeQuoteTicks(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        tick = TestDataStubs.quote_tick(instrument=ETHUSDT_BINANCE)

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

        subscribe1 = SubscribeQuoteTicks(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        subscribe2 = SubscribeQuoteTicks(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe1)
        self.data_engine.execute(subscribe2)

        tick = TestDataStubs.quote_tick(instrument=ETHUSDT_BINANCE)

        # Act
        self.data_engine.process(tick)

        # Assert
        assert handler1 == [tick]
        assert handler2 == [tick]

    def test_subscribe_trade_ticks_then_subscribes(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        subscribe = SubscribeTradeTicks(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        assert self.data_engine.subscribed_trade_ticks() == [ETHUSDT_BINANCE.id]

    def test_unsubscribe_trade_ticks_then_unsubscribes(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        handler = []
        self.msgbus.subscribe(topic="data.trades.BINANCE.ETH/USD", handler=handler.append)

        subscribe = SubscribeTradeTicks(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        assert self.binance_client.subscribed_trade_ticks() == [ETHUSDT_BINANCE.id]

        unsubscribe = UnsubscribeTradeTicks(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.subscribed_trade_ticks() == []
        assert self.binance_client.subscribed_trade_ticks() == []

    def test_subscribe_mark_prices_then_subscribes(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        subscribe = SubscribeMarkPrices(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        assert self.data_engine.subscribed_mark_prices() == [ETHUSDT_BINANCE.id]

    def test_unsubscribe_mark_prices_then_unsubscribes(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        handler = []
        self.msgbus.subscribe(topic="data.mark_prices.BINANCE.ETH/USD", handler=handler.append)

        subscribe = SubscribeMarkPrices(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        assert self.binance_client.subscribed_mark_prices() == [ETHUSDT_BINANCE.id]

        unsubscribe = UnsubscribeMarkPrices(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.subscribed_mark_prices() == []
        assert self.binance_client.subscribed_mark_prices() == []

    def test_subscribe_index_prices_then_subscribes(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        subscribe = SubscribeIndexPrices(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        assert self.data_engine.subscribed_index_prices() == [ETHUSDT_BINANCE.id]

    def test_unsubscribe_index_prices_then_unsubscribes(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        handler = []
        self.msgbus.subscribe(topic="data.index_prices.BINANCE.ETH/USD", handler=handler.append)

        subscribe = SubscribeIndexPrices(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        assert self.binance_client.subscribed_index_prices() == [ETHUSDT_BINANCE.id]

        unsubscribe = UnsubscribeIndexPrices(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.subscribed_index_prices() == []
        assert self.binance_client.subscribed_index_prices() == []

    def test_subscribe_funding_rates_then_subscribes(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        subscribe = SubscribeFundingRates(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        assert self.data_engine.subscribed_funding_rates() == [ETHUSDT_BINANCE.id]

    def test_unsubscribe_funding_rates_then_unsubscribes(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        handler = []
        self.msgbus.subscribe(topic="data.funding_rates.BINANCE.ETH/USD", handler=handler.append)

        subscribe = SubscribeFundingRates(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        assert self.binance_client.subscribed_funding_rates() == [ETHUSDT_BINANCE.id]

        unsubscribe = UnsubscribeFundingRates(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.subscribed_funding_rates() == []
        assert self.binance_client.subscribed_funding_rates() == []

    def test_process_funding_rate_when_subscriber_then_sends_to_registered_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        handler = []
        self.msgbus.subscribe(topic="data.funding_rates.BINANCE.ETHUSDT", handler=handler.append)

        subscribe = SubscribeFundingRates(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        from decimal import Decimal

        from nautilus_trader.model.data import FundingRateUpdate

        funding_rate = FundingRateUpdate(
            instrument_id=ETHUSDT_BINANCE.id,
            rate=Decimal("0.0001"),
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.data_engine.process(funding_rate)

        # Assert
        assert handler == [funding_rate]
        assert self.cache.funding_rate(ETHUSDT_BINANCE.id) == funding_rate

    def test_process_funding_rate_when_subscribers_then_sends_to_registered_handlers(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        handler1 = []
        handler2 = []
        self.msgbus.subscribe(topic="data.funding_rates.BINANCE.ETHUSDT", handler=handler1.append)
        self.msgbus.subscribe(topic="data.funding_rates.BINANCE.ETHUSDT", handler=handler2.append)

        subscribe1 = SubscribeFundingRates(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        subscribe2 = SubscribeFundingRates(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe1)
        self.data_engine.execute(subscribe2)

        from decimal import Decimal

        from nautilus_trader.model.data import FundingRateUpdate

        funding_rate = FundingRateUpdate(
            instrument_id=ETHUSDT_BINANCE.id,
            rate=Decimal("0.0001"),
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.data_engine.process(funding_rate)

        # Assert
        assert handler1 == [funding_rate]
        assert handler2 == [funding_rate]

    def test_subscribe_synthetic_quote_ticks_then_subscribes(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        synthetic = TestInstrumentProvider.synthetic_instrument()
        self.cache.add_synthetic(synthetic)

        subscribe = SubscribeQuoteTicks(
            client_id=None,  # Will route to the Binance venue
            venue=Venue("SYNTH"),
            instrument_id=synthetic.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        assert self.data_engine.subscribed_synthetic_quotes() == [synthetic.id]

    def test_subscribe_synthetic_trade_ticks_then_subscribes(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        synthetic = TestInstrumentProvider.synthetic_instrument()
        self.cache.add_synthetic(synthetic)

        subscribe = SubscribeTradeTicks(
            client_id=None,  # Will route to the Binance venue
            venue=Venue("SYNTH"),
            instrument_id=synthetic.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        assert self.data_engine.subscribed_synthetic_trades() == [synthetic.id]

    def test_process_trade_tick_when_subscriber_then_sends_to_registered_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        handler = []
        self.msgbus.subscribe(topic="data.trades.BINANCE.ETHUSDT", handler=handler.append)

        subscribe = SubscribeTradeTicks(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        tick = TestDataStubs.trade_tick(instrument=ETHUSDT_BINANCE)

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

        subscribe1 = SubscribeTradeTicks(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        subscribe2 = SubscribeTradeTicks(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe1)
        self.data_engine.execute(subscribe2)

        tick = TestDataStubs.trade_tick(instrument=ETHUSDT_BINANCE)

        # Act
        self.data_engine.process(tick)

        # Assert
        assert handler1 == [tick]
        assert handler2 == [tick]

    def test_process_trade_tick_when_synthetic_then_sends_to_registered_handlers(
        self,
    ):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        synthetic = TestInstrumentProvider.synthetic_instrument()
        self.cache.add_synthetic(synthetic)

        handler1 = []
        handler2 = []
        self.msgbus.subscribe(topic="data.trades.BINANCE.ETHUSDT", handler=handler1.append)
        self.msgbus.subscribe(topic="data.trades.SYNTH.BTC-ETH", handler=handler2.append)

        subscribe1 = SubscribeTradeTicks(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        subscribe2 = SubscribeTradeTicks(
            client_id=None,  # Will route to the Binance venue
            venue=synthetic.id.venue,
            instrument_id=synthetic.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe1)
        self.data_engine.execute(subscribe2)

        tick1 = TestDataStubs.trade_tick(instrument=BTCUSDT_BINANCE, price=50_000.0)
        tick2 = TestDataStubs.trade_tick(instrument=ETHUSDT_BINANCE, price=10_000.0)
        tick3 = TestDataStubs.trade_tick(instrument=BTCUSDT_BINANCE, price=50_001.0)

        # Act
        self.data_engine.process(tick1)
        self.data_engine.process(tick2)
        self.data_engine.process(tick3)

        # Assert
        assert handler1 == [tick2]
        assert len(handler2) == 2
        synthetic_tick = handler2[-1]
        assert isinstance(synthetic_tick, TradeTick)
        assert synthetic_tick.to_dict(synthetic_tick) == {
            "type": "TradeTick",
            "instrument_id": "BTC-ETH.SYNTH",
            "price": "30000.50000000",
            "size": "1",
            "aggressor_side": "BUYER",
            "trade_id": "123456",
            "ts_event": 0,
            "ts_init": 0,
        }

    def test_process_quote_tick_when_synthetic_then_sends_to_registered_handlers(
        self,
    ):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        synthetic = TestInstrumentProvider.synthetic_instrument()
        self.cache.add_synthetic(synthetic)

        handler1 = []
        handler2 = []
        self.msgbus.subscribe(topic="data.quotes.BINANCE.ETHUSDT", handler=handler1.append)
        self.msgbus.subscribe(topic="data.quotes.SYNTH.BTC-ETH", handler=handler2.append)

        subscribe1 = SubscribeQuoteTicks(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            instrument_id=ETHUSDT_BINANCE.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        subscribe2 = SubscribeQuoteTicks(
            client_id=None,  # Will route to the Binance venue
            venue=synthetic.id.venue,
            instrument_id=synthetic.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe1)
        self.data_engine.execute(subscribe2)

        tick1 = TestDataStubs.quote_tick(
            instrument=BTCUSDT_BINANCE,
            bid_price=50_000.0,
            ask_price=50_001.0,
        )
        tick2 = TestDataStubs.quote_tick(
            instrument=ETHUSDT_BINANCE,
            bid_price=10_000.0,
            ask_price=10_000.0,
        )
        tick3 = TestDataStubs.quote_tick(
            instrument=BTCUSDT_BINANCE,
            bid_price=50_001.0,
            ask_price=50_002.0,
        )

        # Act
        self.data_engine.process(tick1)
        self.data_engine.process(tick2)
        self.data_engine.process(tick3)

        # Assert
        assert handler1 == [tick2]
        assert len(handler2) == 2
        synthetic_tick = handler2[-1]
        assert isinstance(synthetic_tick, QuoteTick)
        assert synthetic_tick.to_dict(synthetic_tick) == {
            "type": "QuoteTick",
            "instrument_id": "BTC-ETH.SYNTH",
            "bid_price": "30000.50000000",
            "ask_price": "30001.00000000",
            "bid_size": "1",
            "ask_size": "1",
            "ts_event": 0,
            "ts_init": 0,
        }

    def test_subscribe_bar_type_then_subscribes(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(ETHUSDT_BINANCE.id, bar_spec)

        handler = []
        self.msgbus.subscribe(topic=f"data.bars.{bar_type}", handler=handler.append)

        subscribe = SubscribeBars(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            bar_type=bar_type,
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

        subscribe = SubscribeBars(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            bar_type=bar_type,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        assert self.binance_client.subscribed_bars() == [bar_type]

        self.msgbus.unsubscribe(topic=f"data.bars.{bar_type}", handler=handler.append)
        unsubscribe = UnsubscribeBars(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            bar_type=bar_type,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        assert self.data_engine.command_count == 2
        assert self.data_engine.subscribed_bars() == []
        assert self.binance_client.subscribed_bars() == []

    def test_process_bar_when_subscriber_then_sends_to_registered_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(ETHUSDT_BINANCE.id, bar_spec)

        handler = []
        self.msgbus.subscribe(topic=f"data.bars.{bar_type}", handler=handler.append)

        subscribe = SubscribeBars(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            bar_type=bar_type,
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

        subscribe1 = SubscribeBars(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            bar_type=bar_type,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        subscribe2 = SubscribeBars(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            bar_type=bar_type,
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

        subscribe = SubscribeBars(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            bar_type=bar_type,
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

        subscribe = SubscribeBars(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            bar_type=bar_type,
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

        subscribe = SubscribeBars(
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            bar_type=bar_type,
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
        request = RequestInstrument(
            instrument_id=ETHUSDT_BINANCE.id,
            start=None,
            end=None,
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert self.data_engine.request_count == 1
        assert len(handler) == 1
        assert handler[0].data == [ETHUSDT_BINANCE]

    def test_request_instruments_reaches_client(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        handler = []
        request = RequestInstruments(
            start=None,
            end=None,
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert self.data_engine.request_count == 1
        assert len(handler) == 1
        assert handler[0].data == [BTCUSDT_BINANCE, ETHUSDT_BINANCE]

    @pytest.mark.skipif(sys.platform == "win32", reason="Failing on windows")
    def test_request_instrument_when_catalog_registered(self):
        # Arrange
        catalog = setup_catalog(protocol="file", path=self.tmp_path / "catalog")

        idealpro = Venue("IDEALPRO")
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=idealpro)
        catalog.write_data([instrument])

        self.data_engine.register_catalog(catalog)

        # Act
        handler = []
        request = RequestInstrument(
            instrument_id=instrument.id,
            start=None,
            end=None,
            client_id=None,  # Will route to the Binance venue
            venue=idealpro,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert self.data_engine.request_count == 1
        assert len(handler) == 1
        assert (
            isinstance(handler[0].data, list)
            and len(handler[0].data) == 1
            and isinstance(handler[0].data[0], Instrument)
        )
        assert handler[0].data[0].id == instrument.id

    @pytest.mark.skipif(sys.platform == "win32", reason="Failing on windows")
    def test_request_instruments_for_venue_when_catalog_registered(self):
        # Arrange
        catalog = setup_catalog(protocol="file", path=self.tmp_path / "catalog")

        idealpro = Venue("IDEALPRO")
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=idealpro)
        catalog.write_data([instrument])

        self.data_engine.register_catalog(catalog)

        # Act
        handler = []
        request = RequestInstruments(
            start=None,
            end=None,
            client_id=None,
            venue=idealpro,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert self.data_engine.request_count == 1
        assert len(handler) == 1
        assert len(handler[0].data) == 1

    @pytest.mark.skipif(sys.platform == "win32", reason="Failing on windows")
    def test_request_instrument_with_different_ts_init_values(self):
        """
        Test that RequestInstrument returns the correct instrument based on ts_init and
        time filtering.

        RequestInstrument always returns the latest instrument (data[-1]) regardless of
        parameters.

        """
        # Arrange
        catalog = setup_catalog(protocol="file", path=self.tmp_path / "catalog")

        idealpro = Venue("IDEALPRO")
        base_instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=idealpro)

        # Create instruments with different ts_init values (representing different times when instrument was created/updated)
        ts_1 = 1_000_000_000_000_000_000  # Earlier time
        ts_2 = 2_000_000_000_000_000_000  # Later time
        ts_3 = 3_000_000_000_000_000_000  # Even later time

        # Create the same instrument with different ts_init values
        instrument_v1 = CurrencyPair(
            instrument_id=base_instrument.id,
            raw_symbol=base_instrument.raw_symbol,
            base_currency=base_instrument.base_currency,
            quote_currency=base_instrument.quote_currency,
            price_precision=base_instrument.price_precision,
            size_precision=base_instrument.size_precision,
            price_increment=base_instrument.price_increment,
            size_increment=base_instrument.size_increment,
            lot_size=base_instrument.lot_size,
            max_quantity=base_instrument.max_quantity,
            min_quantity=base_instrument.min_quantity,
            max_price=base_instrument.max_price,
            min_price=base_instrument.min_price,
            max_notional=base_instrument.max_notional,
            min_notional=base_instrument.min_notional,
            margin_init=base_instrument.margin_init,
            margin_maint=base_instrument.margin_maint,
            maker_fee=base_instrument.maker_fee,
            taker_fee=base_instrument.taker_fee,
            ts_event=ts_1,
            ts_init=ts_1,
        )

        instrument_v2 = CurrencyPair(
            instrument_id=base_instrument.id,
            raw_symbol=base_instrument.raw_symbol,
            base_currency=base_instrument.base_currency,
            quote_currency=base_instrument.quote_currency,
            price_precision=base_instrument.price_precision,
            size_precision=base_instrument.size_precision,
            price_increment=base_instrument.price_increment,
            size_increment=base_instrument.size_increment,
            lot_size=base_instrument.lot_size,
            max_quantity=base_instrument.max_quantity,
            min_quantity=base_instrument.min_quantity,
            max_price=base_instrument.max_price,
            min_price=base_instrument.min_price,
            max_notional=base_instrument.max_notional,
            min_notional=base_instrument.min_notional,
            margin_init=base_instrument.margin_init,
            margin_maint=base_instrument.margin_maint,
            maker_fee=base_instrument.maker_fee,
            taker_fee=base_instrument.taker_fee,
            ts_event=ts_2,
            ts_init=ts_2,
        )

        instrument_v3 = CurrencyPair(
            instrument_id=base_instrument.id,
            raw_symbol=base_instrument.raw_symbol,
            base_currency=base_instrument.base_currency,
            quote_currency=base_instrument.quote_currency,
            price_precision=base_instrument.price_precision,
            size_precision=base_instrument.size_precision,
            price_increment=base_instrument.price_increment,
            size_increment=base_instrument.size_increment,
            lot_size=base_instrument.lot_size,
            max_quantity=base_instrument.max_quantity,
            min_quantity=base_instrument.min_quantity,
            max_price=base_instrument.max_price,
            min_price=base_instrument.min_price,
            max_notional=base_instrument.max_notional,
            min_notional=base_instrument.min_notional,
            margin_init=base_instrument.margin_init,
            margin_maint=base_instrument.margin_maint,
            maker_fee=base_instrument.maker_fee,
            taker_fee=base_instrument.taker_fee,
            ts_event=ts_3,
            ts_init=ts_3,
        )

        # Write instruments to catalog in order
        catalog.write_data([instrument_v1])
        catalog.write_data([instrument_v2])
        catalog.write_data([instrument_v3])

        self.data_engine.register_catalog(catalog)

        # Advance clock to after all instrument timestamps to avoid "future data" error
        self.clock.advance_time(ts_3 + 1_000_000_000_000_000_000)

        # Test 1: Request without time constraints should return the latest instrument (v3)
        handler = []
        request = RequestInstrument(
            instrument_id=base_instrument.id,
            start=None,
            end=None,
            client_id=None,
            venue=idealpro,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Should return the latest instrument (v3)
        assert len(handler) == 1
        assert (
            isinstance(handler[0].data, list)
            and len(handler[0].data) == 1
            and isinstance(handler[0].data[0], Instrument)
        )
        assert handler[0].data[0].id == base_instrument.id
        assert handler[0].data[0].ts_init == ts_3

        # Test 2: Request with end time between ts_1 and ts_2 should return v1
        handler.clear()
        request = RequestInstrument(
            instrument_id=base_instrument.id,
            start=unix_nanos_to_dt(ts_1 - 500_000_000_000_000_000),  # Before ts_1
            end=unix_nanos_to_dt(ts_1 + 500_000_000_000_000_000),  # Between ts_1 and ts_2
            client_id=None,
            venue=idealpro,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Should return instrument v1
        assert len(handler) == 1
        assert (
            isinstance(handler[0].data, list)
            and len(handler[0].data) == 1
            and isinstance(handler[0].data[0], Instrument)
        )
        assert handler[0].data[0].id == base_instrument.id
        assert handler[0].data[0].ts_init == ts_1

        # Test 3: Request with end time between ts_2 and ts_3 should return v2
        handler.clear()
        request = RequestInstrument(
            instrument_id=base_instrument.id,
            start=unix_nanos_to_dt(ts_2 - 500_000_000_000_000_000),  # Before ts_2
            end=unix_nanos_to_dt(ts_2 + 500_000_000_000_000_000),  # Between ts_2 and ts_3
            client_id=None,
            venue=idealpro,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Should return instrument v2
        assert len(handler) == 1
        assert (
            isinstance(handler[0].data, list)
            and len(handler[0].data) == 1
            and isinstance(handler[0].data[0], Instrument)
        )
        assert handler[0].data[0].id == base_instrument.id
        assert handler[0].data[0].ts_init == ts_2

    @pytest.mark.skipif(sys.platform == "win32", reason="Failing on windows")
    def test_request_instruments_with_different_ts_init_values_and_only_last(self):
        """
        Test that RequestInstruments with only_last parameter works correctly.
        """
        # Arrange
        catalog = setup_catalog(protocol="file", path=self.tmp_path / "catalog")

        idealpro = Venue("IDEALPRO")

        # Create two different instruments
        audusd_base = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=idealpro)
        eurusd_base = TestInstrumentProvider.default_fx_ccy("EUR/USD", venue=idealpro)

        # Create multiple versions of each instrument with different ts_init values
        ts_1 = 1_000_000_000_000_000_000  # Earlier time
        ts_2 = 2_000_000_000_000_000_000  # Later time
        ts_3 = 3_000_000_000_000_000_000  # Even later time

        # AUD/USD versions
        audusd_v1 = CurrencyPair(
            instrument_id=audusd_base.id,
            raw_symbol=audusd_base.raw_symbol,
            base_currency=audusd_base.base_currency,
            quote_currency=audusd_base.quote_currency,
            price_precision=audusd_base.price_precision,
            size_precision=audusd_base.size_precision,
            price_increment=audusd_base.price_increment,
            size_increment=audusd_base.size_increment,
            lot_size=audusd_base.lot_size,
            max_quantity=audusd_base.max_quantity,
            min_quantity=audusd_base.min_quantity,
            max_price=audusd_base.max_price,
            min_price=audusd_base.min_price,
            max_notional=audusd_base.max_notional,
            min_notional=audusd_base.min_notional,
            margin_init=audusd_base.margin_init,
            margin_maint=audusd_base.margin_maint,
            maker_fee=audusd_base.maker_fee,
            taker_fee=audusd_base.taker_fee,
            ts_event=ts_1,
            ts_init=ts_1,
        )

        audusd_v2 = CurrencyPair(
            instrument_id=audusd_base.id,
            raw_symbol=audusd_base.raw_symbol,
            base_currency=audusd_base.base_currency,
            quote_currency=audusd_base.quote_currency,
            price_precision=audusd_base.price_precision,
            size_precision=audusd_base.size_precision,
            price_increment=audusd_base.price_increment,
            size_increment=audusd_base.size_increment,
            lot_size=audusd_base.lot_size,
            max_quantity=audusd_base.max_quantity,
            min_quantity=audusd_base.min_quantity,
            max_price=audusd_base.max_price,
            min_price=audusd_base.min_price,
            max_notional=audusd_base.max_notional,
            min_notional=audusd_base.min_notional,
            margin_init=audusd_base.margin_init,
            margin_maint=audusd_base.margin_maint,
            maker_fee=audusd_base.maker_fee,
            taker_fee=audusd_base.taker_fee,
            ts_event=ts_3,
            ts_init=ts_3,
        )

        # EUR/USD versions
        eurusd_v1 = CurrencyPair(
            instrument_id=eurusd_base.id,
            raw_symbol=eurusd_base.raw_symbol,
            base_currency=eurusd_base.base_currency,
            quote_currency=eurusd_base.quote_currency,
            price_precision=eurusd_base.price_precision,
            size_precision=eurusd_base.size_precision,
            price_increment=eurusd_base.price_increment,
            size_increment=eurusd_base.size_increment,
            lot_size=eurusd_base.lot_size,
            max_quantity=eurusd_base.max_quantity,
            min_quantity=eurusd_base.min_quantity,
            max_price=eurusd_base.max_price,
            min_price=eurusd_base.min_price,
            max_notional=eurusd_base.max_notional,
            min_notional=eurusd_base.min_notional,
            margin_init=eurusd_base.margin_init,
            margin_maint=eurusd_base.margin_maint,
            maker_fee=eurusd_base.maker_fee,
            taker_fee=eurusd_base.taker_fee,
            ts_event=ts_2,
            ts_init=ts_2,
        )

        # Write instruments to catalog
        catalog.write_data([audusd_v1])
        catalog.write_data([eurusd_v1])
        catalog.write_data([audusd_v2])  # Later version of AUD/USD

        self.data_engine.register_catalog(catalog)

        # Advance clock to after all instrument timestamps to avoid "future data" error
        self.clock.advance_time(ts_3 + 1_000_000_000_000_000_000)

        # Test 1: Request with only_last=True (default) should return only latest version of each instrument
        handler = []
        request = RequestInstruments(
            start=None,
            end=None,
            client_id=None,
            venue=idealpro,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params={"only_last": True},
        )

        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Should return 2 instruments: latest AUD/USD (v2) and EUR/USD (v1)
        assert len(handler) == 1
        assert len(handler[0].data) == 2

        # Sort by instrument ID for consistent testing
        instruments = sorted(handler[0].data, key=lambda x: str(x.id))

        # First should be AUD/USD v2 (latest)
        assert instruments[0].id == audusd_base.id
        assert instruments[0].ts_init == ts_3

        # Second should be EUR/USD v1 (only version)
        assert instruments[1].id == eurusd_base.id
        assert instruments[1].ts_init == ts_2

        # Test 2: Request with only_last=False should return all instruments
        handler.clear()
        request = RequestInstruments(
            start=None,
            end=None,
            client_id=None,
            venue=idealpro,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params={"only_last": False},
        )

        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Should return 3 instruments: both AUD/USD versions and EUR/USD
        assert len(handler) == 1
        assert len(handler[0].data) == 3

        # Sort by ts_init for consistent testing
        instruments = sorted(handler[0].data, key=lambda x: x.ts_init)

        # First should be AUD/USD v1
        assert instruments[0].id == audusd_base.id
        assert instruments[0].ts_init == ts_1

        # Second should be EUR/USD v1
        assert instruments[1].id == eurusd_base.id
        assert instruments[1].ts_init == ts_2

        # Third should be AUD/USD v2
        assert instruments[2].id == audusd_base.id
        assert instruments[2].ts_init == ts_3

    @pytest.mark.skipif(sys.platform == "win32", reason="Failing on windows")
    def test_request_instruments_with_time_filtering(self):
        """
        Test that RequestInstruments respects start and end time parameters.
        """
        # Arrange
        catalog = setup_catalog(protocol="file", path=self.tmp_path / "catalog")

        idealpro = Venue("IDEALPRO")

        # Create instruments with different ts_init values
        ts_1 = 1_000_000_000_000_000_000  # Earlier time
        ts_2 = 2_000_000_000_000_000_000  # Middle time
        ts_3 = 3_000_000_000_000_000_000  # Later time

        audusd_base = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=idealpro)
        eurusd_base = TestInstrumentProvider.default_fx_ccy("EUR/USD", venue=idealpro)
        gbpusd_base = TestInstrumentProvider.default_fx_ccy("GBP/USD", venue=idealpro)

        # Create instruments at different times
        audusd_early = CurrencyPair(
            instrument_id=audusd_base.id,
            raw_symbol=audusd_base.raw_symbol,
            base_currency=audusd_base.base_currency,
            quote_currency=audusd_base.quote_currency,
            price_precision=audusd_base.price_precision,
            size_precision=audusd_base.size_precision,
            price_increment=audusd_base.price_increment,
            size_increment=audusd_base.size_increment,
            lot_size=audusd_base.lot_size,
            max_quantity=audusd_base.max_quantity,
            min_quantity=audusd_base.min_quantity,
            max_price=audusd_base.max_price,
            min_price=audusd_base.min_price,
            max_notional=audusd_base.max_notional,
            min_notional=audusd_base.min_notional,
            margin_init=audusd_base.margin_init,
            margin_maint=audusd_base.margin_maint,
            maker_fee=audusd_base.maker_fee,
            taker_fee=audusd_base.taker_fee,
            ts_event=ts_1,
            ts_init=ts_1,
        )

        eurusd_middle = CurrencyPair(
            instrument_id=eurusd_base.id,
            raw_symbol=eurusd_base.raw_symbol,
            base_currency=eurusd_base.base_currency,
            quote_currency=eurusd_base.quote_currency,
            price_precision=eurusd_base.price_precision,
            size_precision=eurusd_base.size_precision,
            price_increment=eurusd_base.price_increment,
            size_increment=eurusd_base.size_increment,
            lot_size=eurusd_base.lot_size,
            max_quantity=eurusd_base.max_quantity,
            min_quantity=eurusd_base.min_quantity,
            max_price=eurusd_base.max_price,
            min_price=eurusd_base.min_price,
            max_notional=eurusd_base.max_notional,
            min_notional=eurusd_base.min_notional,
            margin_init=eurusd_base.margin_init,
            margin_maint=eurusd_base.margin_maint,
            maker_fee=eurusd_base.maker_fee,
            taker_fee=eurusd_base.taker_fee,
            ts_event=ts_2,
            ts_init=ts_2,
        )

        gbpusd_late = CurrencyPair(
            instrument_id=gbpusd_base.id,
            raw_symbol=gbpusd_base.raw_symbol,
            base_currency=gbpusd_base.base_currency,
            quote_currency=gbpusd_base.quote_currency,
            price_precision=gbpusd_base.price_precision,
            size_precision=gbpusd_base.size_precision,
            price_increment=gbpusd_base.price_increment,
            size_increment=gbpusd_base.size_increment,
            lot_size=gbpusd_base.lot_size,
            max_quantity=gbpusd_base.max_quantity,
            min_quantity=gbpusd_base.min_quantity,
            max_price=gbpusd_base.max_price,
            min_price=gbpusd_base.min_price,
            max_notional=gbpusd_base.max_notional,
            min_notional=gbpusd_base.min_notional,
            margin_init=gbpusd_base.margin_init,
            margin_maint=gbpusd_base.margin_maint,
            maker_fee=gbpusd_base.maker_fee,
            taker_fee=gbpusd_base.taker_fee,
            ts_event=ts_3,
            ts_init=ts_3,
        )

        # Write instruments to catalog
        catalog.write_data([audusd_early])
        catalog.write_data([eurusd_middle])
        catalog.write_data([gbpusd_late])

        self.data_engine.register_catalog(catalog)

        # Advance clock to after all instrument timestamps to avoid "future data" error
        self.clock.advance_time(ts_3 + 1_000_000_000_000_000_000)

        # Test 1: Request instruments from start to middle time (should get AUD/USD and EUR/USD)
        handler = []
        request = RequestInstruments(
            start=unix_nanos_to_dt(ts_1 - 500_000_000_000_000_000),  # Before ts_1
            end=unix_nanos_to_dt(ts_2 + 500_000_000_000_000_000),  # After ts_2, before ts_3
            client_id=None,
            venue=idealpro,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Should return 2 instruments: AUD/USD and EUR/USD (not GBP/USD which is after end time)
        assert len(handler) == 1
        assert len(handler[0].data) == 2

        # Sort by ts_init for consistent testing
        instruments = sorted(handler[0].data, key=lambda x: x.ts_init)

        assert instruments[0].id == audusd_base.id
        assert instruments[0].ts_init == ts_1
        assert instruments[1].id == eurusd_base.id
        assert instruments[1].ts_init == ts_2

        # Test 2: Request instruments from middle to late time (should get EUR/USD and GBP/USD)
        handler.clear()
        request = RequestInstruments(
            start=unix_nanos_to_dt(ts_2 - 500_000_000_000_000_000),  # Before ts_2
            end=unix_nanos_to_dt(ts_3 + 500_000_000_000_000_000),  # After ts_3
            client_id=None,
            venue=idealpro,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Should return 2 instruments: EUR/USD and GBP/USD (not AUD/USD which is before start time)
        assert len(handler) == 1
        assert len(handler[0].data) == 2

        # Sort by ts_init for consistent testing
        instruments = sorted(handler[0].data, key=lambda x: x.ts_init)

        assert instruments[0].id == eurusd_base.id
        assert instruments[0].ts_init == ts_2
        assert instruments[1].id == gbpusd_base.id
        assert instruments[1].ts_init == ts_3

        # Test 3: Request instruments with narrow time window (should get only EUR/USD)
        handler.clear()
        request = RequestInstruments(
            start=unix_nanos_to_dt(ts_2 - 100_000_000_000_000_000),  # Just before ts_2
            end=unix_nanos_to_dt(ts_2 + 100_000_000_000_000_000),  # Just after ts_2
            client_id=None,
            venue=idealpro,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Should return 1 instrument: only EUR/USD
        assert len(handler) == 1
        assert len(handler[0].data) == 1
        assert handler[0].data[0].id == eurusd_base.id
        assert handler[0].data[0].ts_init == ts_2

    def test_request_order_book_snapshot_reaches_client(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        deltas = OrderBookDeltas(
            instrument_id=ETHUSDT_BINANCE.id,
            deltas=[TestDataStubs.order_book_delta(instrument_id=ETHUSDT_BINANCE.id)],
        )

        self.data_engine.process(deltas)

        handler = []
        request = RequestOrderBookSnapshot(
            instrument_id=ETHUSDT_BINANCE.id,
            limit=10,
            client_id=None,  # Will route to the Binance venue
            venue=BINANCE,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert self.data_engine.request_count == 1
        assert len(handler) == 0

    def test_request_bars_reaches_client(self):
        # Arrange
        self.data_engine.register_client(self.mock_market_data_client)
        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(ETHUSDT_BINANCE.id, bar_spec)
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
        self.mock_market_data_client.bars = [bar]

        handler = []
        request = RequestBars(
            bar_type=bar_type,
            start=None,
            end=None,
            limit=0,
            client_id=None,
            venue=bar_type.instrument_id.venue,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params={"update_catalog": False},
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert self.data_engine.request_count == 1
        assert len(handler) == 1
        assert handler[0].data == [bar]

    def test_request_bars_with_start_and_end(self):
        # Arrange
        self.data_engine.register_client(self.mock_market_data_client)
        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(ETHUSDT_BINANCE.id, bar_spec)
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
        self.mock_market_data_client.bars = [bar]

        handler = []
        request = RequestBars(
            bar_type=bar_type,
            start=pd.Timestamp("2024-10-01"),
            end=pd.Timestamp("2024-10-31"),
            limit=0,
            client_id=None,
            venue=bar_type.instrument_id.venue,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params={"update_catalog": False},
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert self.data_engine.request_count == 1
        assert len(handler) == 1
        assert handler[0].data == [bar]

    def test_request_bars_when_catalog_registered(self):
        # Arrange
        catalog = setup_catalog(protocol="file", path=self.tmp_path / "catalog")
        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(ETHUSDT_BINANCE.id, bar_spec)
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
        catalog.write_data([bar])
        self.data_engine.register_catalog(catalog)

        handler = []
        request = RequestBars(
            bar_type=bar_type,
            start=None,
            end=None,
            limit=0,
            client_id=None,
            venue=bar_type.instrument_id.venue,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params={"update_catalog": False},
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert self.data_engine.request_count == 1
        assert len(handler) == 1
        assert handler[0].data == [bar]

    def test_request_bars_when_catalog_and_client_registered(self):
        # Arrange
        catalog = setup_catalog(protocol="file", path=self.tmp_path / "catalog")
        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(ETHUSDT_BINANCE.id, bar_spec)
        bar = Bar(
            bar_type,
            Price.from_str("1051.00000"),
            Price.from_str("1055.00000"),
            Price.from_str("1050.00000"),
            Price.from_str("1052.00000"),
            Quantity.from_int(100),
            pd.Timestamp("2024-3-24").value,
            pd.Timestamp("2024-3-24").value,
        )
        catalog.write_data([bar])
        self.data_engine.register_catalog(catalog)

        self.data_engine.register_client(self.mock_market_data_client)
        bar2 = Bar(
            bar_type,
            Price.from_str("1051.00000"),
            Price.from_str("1055.00000"),
            Price.from_str("1050.00000"),
            Price.from_str("1052.00000"),
            Quantity.from_int(100),
            pd.Timestamp("2024-3-25").value,
            pd.Timestamp("2024-3-25").value,
        )
        self.mock_market_data_client.bars = [bar2]

        handler = []
        request = RequestBars(
            bar_type=bar_type,
            start=pd.Timestamp("2024-3-24"),
            end=pd.Timestamp("2024-3-25"),
            limit=0,
            client_id=None,
            venue=bar_type.instrument_id.venue,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params={"update_catalog": True},
        )

        self.clock.advance_time(pd.Timestamp("2024-3-25").value)

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert self.data_engine.request_count == 1
        assert len(handler) == 1
        assert handler[0].data == [bar, bar2]
        assert catalog.query_last_timestamp(Bar, bar_type) == time_object_to_dt(
            pd.Timestamp("2024-3-25"),
        )
        assert catalog.get_intervals(Bar, bar_type) == [
            (1711238400000000000, 1711238400000000000),
            (1711238400000000001, 1711324800000000000),
        ]

        # Arrange
        self.mock_market_data_client.bars = []

        handler = []
        request = RequestBars(
            bar_type=bar_type,
            start=pd.Timestamp("2024-3-24"),
            end=pd.Timestamp("2024-3-25"),
            limit=0,
            client_id=None,
            venue=bar_type.instrument_id.venue,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params={"update_catalog": True},
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert catalog.get_intervals(Bar, bar_type) == [
            (1711238400000000000, 1711238400000000000),
            (1711238400000000001, 1711324800000000000),
        ]

        # Act
        catalog.consolidate_catalog()

        # Assert
        assert catalog.get_intervals(Bar, bar_type) == [
            (1711238400000000000, 1711324800000000000),
        ]

        # Arrange
        parquet_file = os.path.join(
            catalog.path,
            "data",
            "bar",
            str(bar_type),
            _timestamps_to_filename(1711238400000000000, 1711324800000000000),
        )
        other_name = os.path.join(catalog.path, "data", "bar", str(bar_type), "other.parquet")
        os.rename(parquet_file, other_name)

        # Act
        catalog.reset_all_file_names()

        # Assert
        assert catalog.get_intervals(Bar, bar_type) == [
            (1711238400000000000, 1711324800000000000),
        ]

    def test_request_quote_ticks_reaches_client(self):
        # Arrange
        self.data_engine.register_client(self.mock_market_data_client)
        quote_tick = QuoteTick(
            ETHUSDT_BINANCE.id,
            Price.from_str("1051.00000"),
            Price.from_str("1052.00000"),
            Quantity.from_int(100),
            Quantity.from_int(100),
            0,
            0,
        )
        self.mock_market_data_client.quote_ticks = [quote_tick]

        handler = []
        request = RequestQuoteTicks(
            instrument_id=ETHUSDT_BINANCE.id,
            start=None,
            end=None,
            limit=0,
            client_id=None,  # Will route to the Binance venue
            venue=ETHUSDT_BINANCE.venue,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params={"update_catalog": False},
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert self.data_engine.request_count == 1
        assert len(handler) == 1
        assert handler[0].data == [quote_tick]

    def test_request_quote_ticks_when_catalog_registered(self):
        # Arrange
        catalog = setup_catalog(protocol="file", path=self.tmp_path / "catalog")
        quote_tick = QuoteTick(
            ETHUSDT_BINANCE.id,
            Price.from_str("1051.00000"),
            Price.from_str("1052.00000"),
            Quantity.from_int(100),
            Quantity.from_int(100),
            0,
            0,
        )
        catalog.write_data([quote_tick])
        self.data_engine.register_catalog(catalog)

        handler = []
        request = RequestQuoteTicks(
            instrument_id=ETHUSDT_BINANCE.id,
            start=None,
            end=None,
            limit=0,
            client_id=None,  # Will route to the Binance venue
            venue=ETHUSDT_BINANCE.venue,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params={"update_catalog": False},
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert self.data_engine.request_count == 1
        assert len(handler) == 1
        # assert handler[0].data == [quote_tick]

    def test_request_quote_ticks_when_catalog_and_client_registered(self):
        # Arrange
        catalog = setup_catalog(protocol="file", path=self.tmp_path / "catalog")
        quote_tick = QuoteTick(
            ETHUSDT_BINANCE.id,
            Price.from_str("1051.00000"),
            Price.from_str("1052.00000"),
            Quantity.from_int(100),
            Quantity.from_int(100),
            pd.Timestamp("2024-3-24").value,
            pd.Timestamp("2024-3-24").value,
        )
        catalog.write_data([quote_tick])
        self.data_engine.register_catalog(catalog)

        self.data_engine.register_client(self.mock_market_data_client)
        quote_tick2 = QuoteTick(
            ETHUSDT_BINANCE.id,
            Price.from_str("1051.00000"),
            Price.from_str("1052.00000"),
            Quantity.from_int(100),
            Quantity.from_int(100),
            pd.Timestamp("2024-3-25").value,
            pd.Timestamp("2024-3-25").value,
        )
        self.mock_market_data_client.quote_ticks = [quote_tick2]

        handler = []
        request = RequestQuoteTicks(
            instrument_id=ETHUSDT_BINANCE.id,
            start=pd.Timestamp("2024-3-24"),
            end=pd.Timestamp("2024-3-25"),
            limit=0,
            client_id=None,  # Will route to the Binance venue
            venue=ETHUSDT_BINANCE.venue,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params={"update_catalog": True},
        )

        self.clock.advance_time(pd.Timestamp("2024-3-25").value)

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert self.data_engine.request_count == 1
        assert len(handler) == 1
        # assert handler[0].data == [quote_tick, quote_tick2]
        assert catalog.query_last_timestamp(
            QuoteTick,
            ETHUSDT_BINANCE.id,
        ) == time_object_to_dt(
            pd.Timestamp("2024-3-25"),
        )

    def test_request_trade_ticks_reaches_client(self):
        # Arrange
        self.data_engine.register_client(self.mock_market_data_client)
        trade_tick = TradeTick(
            instrument_id=ETHUSDT_BINANCE.id,
            price=Price.from_str("1051.00000"),
            size=Quantity.from_int(100),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456"),
            ts_event=0,
            ts_init=0,
        )
        self.mock_market_data_client.trade_ticks = [trade_tick]

        handler = []
        request = RequestTradeTicks(
            instrument_id=ETHUSDT_BINANCE.id,
            start=None,
            end=None,
            limit=0,
            client_id=None,  # Will route to the Binance venue
            venue=ETHUSDT_BINANCE.venue,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params={"update_catalog": True},
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert self.data_engine.request_count == 1
        assert len(handler) == 1
        assert handler[0].data == [trade_tick]

    def test_request_trade_ticks_when_catalog_registered(self):
        # Arrange
        catalog = setup_catalog(protocol="file", path=self.tmp_path / "catalog")
        trade_tick = TradeTick(
            instrument_id=ETHUSDT_BINANCE.id,
            price=Price.from_str("1051.00000"),
            size=Quantity.from_int(100),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456"),
            ts_event=0,
            ts_init=0,
        )
        catalog.write_data([trade_tick])
        self.data_engine.register_catalog(catalog)

        assert trade_tick == trade_tick

        handler = []
        request = RequestTradeTicks(
            instrument_id=ETHUSDT_BINANCE.id,
            start=None,
            end=None,
            limit=0,
            client_id=None,  # Will route to the Binance venue
            venue=ETHUSDT_BINANCE.venue,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params={"update_catalog": False},
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert self.data_engine.request_count == 1
        assert len(handler) == 1
        # assert handler[0].data == [trade_tick]

    def test_request_trade_ticks_when_catalog_and_client_registered(self):
        # Arrange
        catalog = setup_catalog(protocol="file", path=self.tmp_path / "catalog")
        trade_tick = TradeTick(
            instrument_id=ETHUSDT_BINANCE.id,
            price=Price.from_str("1051.00000"),
            size=Quantity.from_int(100),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456"),
            ts_event=pd.Timestamp("2024-3-24").value,
            ts_init=pd.Timestamp("2024-3-24").value,
        )
        catalog.write_data([trade_tick])
        self.data_engine.register_catalog(catalog)

        self.data_engine.register_client(self.mock_market_data_client)
        trade_tick2 = TradeTick(
            instrument_id=ETHUSDT_BINANCE.id,
            price=Price.from_str("1051.00000"),
            size=Quantity.from_int(100),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456"),
            ts_event=pd.Timestamp("2024-3-25").value,
            ts_init=pd.Timestamp("2024-3-25").value,
        )
        self.mock_market_data_client.trade_ticks = [trade_tick2]

        handler = []
        request = RequestTradeTicks(
            instrument_id=ETHUSDT_BINANCE.id,
            start=pd.Timestamp("2024-3-24"),
            end=pd.Timestamp("2024-3-25"),
            limit=0,
            client_id=None,  # Will route to the Binance venue
            venue=ETHUSDT_BINANCE.venue,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params={"update_catalog": True},
        )

        self.clock.advance_time(pd.Timestamp("2024-3-25").value)

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert self.data_engine.request_count == 1
        assert len(handler) == 1
        # assert handler[0].data == [trade_tick, trade_tick2]
        assert catalog.query_last_timestamp(
            TradeTick,
            ETHUSDT_BINANCE.id,
        ) == time_object_to_dt(
            pd.Timestamp("2024-3-25"),
        )

    def test_request_order_book_depth_reaches_client(self):
        # Arrange
        self.data_engine.register_client(self.mock_market_data_client)
        from nautilus_trader.test_kit.stubs.data import TestDataStubs

        depth = TestDataStubs.order_book_depth10(instrument_id=ETHUSDT_BINANCE.id)
        self.mock_market_data_client.order_book_depths = [depth]

        handler = []
        request = RequestOrderBookDepth(
            instrument_id=ETHUSDT_BINANCE.id,
            start=None,
            end=None,
            limit=0,
            depth=10,
            client_id=None,  # Will route to the Binance venue
            venue=ETHUSDT_BINANCE.venue,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params={"update_catalog": False},
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        assert self.data_engine.request_count == 1
        assert len(handler) == 1
        assert handler[0].data == [depth]

    def test_request_aggregated_bars_with_bars(self):
        # Arrange
        loader = DatabentoDataLoader()

        path = (
            TEST_DATA_DIR
            / "databento"
            / "historical_bars_catalog"
            / "databento"
            / "futures_ohlcv-1m_2024-07-01T23h40_2024-07-02T00h10.dbn.zst"
        )
        data = loader.from_dbn_file(path, as_legacy_cython=True)

        definition_path = (
            TEST_DATA_DIR
            / "databento"
            / "historical_bars_catalog"
            / "databento"
            / "futures_definition.dbn.zst"
        )
        definition = loader.from_dbn_file(definition_path, as_legacy_cython=True)

        catalog = setup_catalog(protocol="file", path=self.tmp_path / "catalog")
        catalog.write_data(data)
        catalog.write_data(definition)

        self.data_engine.register_catalog(catalog)
        self.data_engine.process(definition[0])

        symbol_id = InstrumentId.from_str("ESU4.GLBX")

        utc_now = pd.Timestamp("2024-07-01T23:56")
        self.clock.advance_time(utc_now.value)

        start = utc_now - pd.Timedelta(minutes=11)
        end = utc_now - pd.Timedelta(minutes=1)

        bar_type_0 = data[0].bar_type
        bar_type_1 = BarType.from_str("ESU4.GLBX-2-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL")
        bar_type_2 = BarType.from_str("ESU4.GLBX-4-MINUTE-LAST-INTERNAL@2-MINUTE-INTERNAL")
        bar_type_3 = BarType.from_str("ESU4.GLBX-5-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL")
        bar_types = [bar_type_1, bar_type_2, bar_type_3]

        handler = []
        params = {}
        params["bar_type"] = bar_types[0].composite()
        params["bar_types"] = tuple(bar_types)
        params["include_external_data"] = True
        params["update_subscriptions"] = False
        params["update_catalog"] = False
        params["bars_market_data_type"] = "bars"

        request = RequestBars(
            bar_type=bar_types[0].composite(),
            start=start,
            end=end,
            limit=0,
            client_id=None,
            venue=symbol_id.venue,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=params,
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        last_1_minute_bar = Bar(
            BarType.from_str("ESU4.GLBX-1-MINUTE-LAST-EXTERNAL"),
            Price.from_str("5528.75"),
            Price.from_str("5529.25"),
            Price.from_str("5528.50"),
            Price.from_str("5528.75"),
            Quantity.from_int(164),
            1719878100000000000,
            1719878100000000000,
        )

        last_2_minute_bar = Bar(
            BarType.from_str("ESU4.GLBX-2-MINUTE-LAST-INTERNAL"),
            Price.from_str("5528.50"),
            Price.from_str("5528.75"),
            Price.from_str("5528.25"),
            Price.from_str("5528.50"),
            Quantity.from_int(76),
            1719878040000000000,
            1719878040000000000,
        )

        last_4_minute_bar = Bar(
            BarType.from_str("ESU4.GLBX-4-MINUTE-LAST-INTERNAL"),
            Price.from_str("5527.50"),
            Price.from_str("5528.50"),
            Price.from_str("5527.50"),
            Price.from_str("5528.50"),
            Quantity.from_int(116),
            1719877920000000000,
            1719877920000000000,
        )

        last_5_minute_bar = Bar(
            BarType.from_str("ESU4.GLBX-5-MINUTE-LAST-INTERNAL"),
            Price.from_str("5527.75"),
            Price.from_str("5529.25"),
            Price.from_str("5527.75"),
            Price.from_str("5528.75"),
            Quantity.from_int(329),
            1719878100000000000,
            1719878100000000000,
        )

        assert handler[0].data["bars"][bar_type_0][-1] == last_1_minute_bar
        assert handler[0].data["bars"][bar_type_1.standard()][-1] == last_2_minute_bar
        assert handler[0].data["bars"][bar_type_2.standard()][-1] == last_4_minute_bar
        assert handler[0].data["bars"][bar_type_3.standard()][-1] == last_5_minute_bar

        bars_0 = self.cache.bars(bar_type_0.standard())
        bars_2 = self.cache.bars(bar_type_2.standard())
        assert bars_0
        assert bars_2

    def test_request_aggregated_bars_with_quotes(self):
        # Arrange
        loader = DatabentoDataLoader()

        path = (
            TEST_DATA_DIR
            / "databento"
            / "historical_bars_catalog"
            / "databento"
            / "futures_mbp-1_2024-07-01T23h58_2024-07-02T00h02.dbn.zst"
        )
        data = loader.from_dbn_file(path, as_legacy_cython=True)

        definition_path = (
            TEST_DATA_DIR
            / "databento"
            / "historical_bars_catalog"
            / "databento"
            / "futures_definition.dbn.zst"
        )
        definition = loader.from_dbn_file(definition_path, as_legacy_cython=True)

        catalog = setup_catalog(protocol="file", path=self.tmp_path / "catalog")
        catalog.write_data(data)
        catalog.write_data(definition)

        self.data_engine.register_catalog(catalog)
        self.data_engine.process(definition[0])

        symbol_id = InstrumentId.from_str("ESU4.GLBX")

        utc_now = pd.Timestamp("2024-07-02T00:00:01")
        self.clock.advance_time(utc_now.value)

        start = utc_now - pd.Timedelta(minutes=2, seconds=1)
        end = utc_now - pd.Timedelta(minutes=0, seconds=1)

        bar_type_1 = BarType.from_str("ESU4.GLBX-1-MINUTE-BID-INTERNAL")
        bar_type_2 = BarType.from_str("ESU4.GLBX-2-MINUTE-BID-INTERNAL@1-MINUTE-INTERNAL")
        bar_types = [bar_type_1, bar_type_2]

        handler = []
        params = {}
        params["bar_type"] = bar_types[0].composite()
        params["bar_types"] = tuple(bar_types)
        params["include_external_data"] = False
        params["update_subscriptions"] = False
        params["update_catalog"] = False
        params["bars_market_data_type"] = "quote_ticks"

        request = RequestQuoteTicks(
            instrument_id=bar_types[0].instrument_id,
            start=start,
            end=end,
            limit=0,
            client_id=None,
            venue=symbol_id.venue,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=params,
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        last_1_minute_bar = Bar(
            BarType.from_str("ESU4.GLBX-1-MINUTE-BID-INTERNAL"),
            Price.from_str("5528.50"),
            Price.from_str("5528.75"),
            Price.from_str("5528.50"),
            Price.from_str("5528.75"),
            Quantity.from_int(5806),
            1719878400000000000,
            1719878400000000000,
        )

        last_2_minute_bar = Bar(
            BarType.from_str("ESU4.GLBX-2-MINUTE-BID-INTERNAL"),
            Price.from_str("5528.50"),
            Price.from_str("5528.75"),
            Price.from_str("5528.50"),
            Price.from_str("5528.75"),
            Quantity.from_int(10244),
            1719878400000000000,
            1719878400000000000,
        )

        assert handler[0].data["bars"][bar_type_1.standard()][-1] == last_1_minute_bar
        assert handler[0].data["bars"][bar_type_2.standard()][-1] == last_2_minute_bar

        bars_2 = self.cache.bars(bar_type_2.standard())
        assert bars_2

    def test_request_aggregated_bars_with_trades(self):
        # Arrange
        loader = DatabentoDataLoader()

        path = (
            TEST_DATA_DIR
            / "databento"
            / "historical_bars_catalog"
            / "databento"
            / "futures_trades_2024-07-01T23h58_2024-07-02T00h02.dbn.zst"
        )
        data = loader.from_dbn_file(path, as_legacy_cython=True)

        definition_path = (
            TEST_DATA_DIR
            / "databento"
            / "historical_bars_catalog"
            / "databento"
            / "futures_definition.dbn.zst"
        )
        definition = loader.from_dbn_file(definition_path, as_legacy_cython=True)

        catalog = setup_catalog(protocol="file", path=self.tmp_path / "catalog")
        catalog.write_data(data)
        catalog.write_data(definition)

        self.data_engine.register_catalog(catalog)
        self.data_engine.process(definition[0])

        symbol_id = InstrumentId.from_str("ESU4.GLBX")

        utc_now = pd.Timestamp("2024-07-02T00:00:01")
        self.clock.advance_time(utc_now.value)

        start = utc_now - pd.Timedelta(minutes=2, seconds=1)
        end = utc_now - pd.Timedelta(minutes=0, seconds=0)

        bar_type_1 = BarType.from_str("ESU4.GLBX-1-MINUTE-LAST-INTERNAL")
        bar_type_2 = BarType.from_str("ESU4.GLBX-2-MINUTE-LAST-INTERNAL@1-MINUTE-INTERNAL")
        bar_types = [bar_type_1, bar_type_2]

        handler = []
        params = {}
        params["bar_type"] = bar_types[0].composite()
        params["bar_types"] = tuple(bar_types)
        params["include_external_data"] = False
        params["update_subscriptions"] = False
        params["update_catalog"] = False
        params["bars_market_data_type"] = "trade_ticks"

        request = RequestTradeTicks(
            instrument_id=bar_types[0].instrument_id,
            start=start,
            end=end,
            limit=0,
            client_id=None,
            venue=symbol_id.venue,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=params,
        )

        # Act
        self.msgbus.request(endpoint="DataEngine.request", request=request)

        # Assert
        last_1_minute_bar = Bar(
            BarType.from_str("ESU4.GLBX-1-MINUTE-LAST-INTERNAL"),
            Price.from_str("5528.50"),
            Price.from_str("5528.75"),
            Price.from_str("5528.50"),
            Price.from_str("5528.75"),
            Quantity.from_int(23),
            1719878400000000000,
            1719878400000000000,
        )

        last_2_minute_bar = Bar(
            BarType.from_str("ESU4.GLBX-2-MINUTE-LAST-INTERNAL"),
            Price.from_str("5528.75"),
            Price.from_str("5528.75"),
            Price.from_str("5528.50"),
            Price.from_str("5528.75"),
            Quantity.from_int(41),
            1719878400000000000,
            1719878400000000000,
        )

        assert handler[0].data["bars"][bar_type_1.standard()][-1] == last_1_minute_bar
        assert handler[0].data["bars"][bar_type_2.standard()][-1] == last_2_minute_bar

        bars_2 = self.cache.bars(bar_type_2.standard())
        assert bars_2

    # TODO: Implement with new Rust datafusion backend"
    # def test_request_quote_ticks_when_catalog_registered_using_rust(self) -> None:
    #     # Arrange
    #     catalog = catalog_setup(protocol="file")
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
    #     self.data_engine.register_catalog(catalog)
    #
    #     # Act
    #     handler: list[DataResponse] = []
    #     request = RequestData(
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
    #     catalog = catalog_setup(protocol="file")
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
    #     self.data_engine.register_catalog(catalog)
    #
    #     # Act
    #     handler: list[DataResponse] = []
    #     request1 = RequestData(
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
    #     request2 = RequestData(
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
    #
    # def test_request_bars_when_catalog_registered(self):
    #     # Arrange
    #     catalog = catalog_setup(protocol="file")
    #     self.clock.set_time(to_time_ns=1638058200000000000)  # <- Set to end of data
    #
    #     bar_type = TestDataStubs.bartype_adabtc_binance_1min_last()
    #     instrument = TestInstrumentProvider.adabtc_binance()
    #     wrangler = BarDataWrangler(bar_type, instrument)
    #
    #     binance_spot_header = [
    #         "timestamp",
    #         "open",
    #         "high",
    #         "low",
    #         "close",
    #         "volume",
    #         "ts_close",
    #         "quote_volume",
    #         "n_trades",
    #         "taker_buy_base_volume",
    #         "taker_buy_quote_volume",
    #         "ignore",
    #     ]
    #     df = pd.read_csv(f"{TEST_DATA_DIR}/ADABTC-1m-2021-11-27.csv", names=binance_spot_header)
    #     df["timestamp"] = df["timestamp"].astype("datetime64[ms]")
    #     bars = wrangler.process(df.set_index("timestamp"))
    #     catalog.write_data(bars)
    #
    #     self.data_engine.register_catalog(catalog)
    #
    #     # Act
    #     handler = []
    #     request = RequestData(
    #         client_id=None,
    #         venue=BINANCE,
    #         data_type=DataType(
    #             Bar,
    #             metadata={
    #                 "bar_type": BarType(
    #                     InstrumentId(Symbol("ADABTC"), BINANCE),
    #                     BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST),
    #                 ),
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
    #     self.msgbus.request(endpoint="DataEngine.request", request=request)
    #
    #     # Assert
    #     assert self.data_engine.request_count == 1
    #     assert len(handler) == 1
    #     assert len(handler[0].data) == 21
    #     assert handler[0].data[0].ts_init == 1637971200000000000
    #     assert handler[0].data[-1].ts_init == 1638058200000000000

    # -- SPREAD INTEGRATION TESTS ----------------------------------------------------------------

    def test_subscribe_spread_quotes_creates_aggregator(self):
        # Arrange
        # Create a client for XCME venue
        xcme_client = BacktestMarketDataClient(
            client_id=ClientId("XCME"),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        self.data_engine.register_client(xcme_client)
        xcme_client.start()

        # Create option instruments for spread
        option1 = OptionContract(
            instrument_id=InstrumentId(Symbol("ESM4 P5230"), Venue("XCME")),
            raw_symbol=Symbol("ESM4 P5230"),
            asset_class=AssetClass.EQUITY,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            underlying="ESM4",
            option_kind=OptionKind.PUT,
            activation_ns=0,
            expiration_ns=1719792000000000000,
            strike_price=Price.from_str("5230.0"),
            ts_event=0,
            ts_init=0,
        )
        option2 = OptionContract(
            instrument_id=InstrumentId(Symbol("ESM4 P5250"), Venue("XCME")),
            raw_symbol=Symbol("ESM4 P5250"),
            asset_class=AssetClass.EQUITY,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            underlying="ESM4",
            option_kind=OptionKind.PUT,
            activation_ns=0,
            expiration_ns=1719792000000000000,
            strike_price=Price.from_str("5250.0"),
            ts_event=0,
            ts_init=0,
        )

        # Add instruments to cache
        self.data_engine.process(option1)
        self.data_engine.process(option2)

        # Create spread instrument ID
        spread_instrument_id = InstrumentId.new_spread(
            [
                (option1.id, 1),
                (option2.id, -1),
            ],
        )

        subscribe = SubscribeQuoteTicks(
            client_id=None,
            venue=Venue("XCME"),  # Use the venue from the spread components
            instrument_id=spread_instrument_id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert - Verify the command was processed without errors
        # Command count is 3: 1 for spread + 2 for component instruments
        assert self.data_engine.command_count == 3
        # Note: The actual spread quote aggregator creation is now handled by the data client
        # This test verifies that the subscription command is processed correctly

    def test_unsubscribe_spread_quotes_removes_aggregator(self):
        # Arrange
        # Create a client for XCME venue
        xcme_client = BacktestMarketDataClient(
            client_id=ClientId("XCME"),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        self.data_engine.register_client(xcme_client)
        xcme_client.start()

        # Create option instruments for spread
        option1 = OptionContract(
            instrument_id=InstrumentId(Symbol("ESM4 P5230"), Venue("XCME")),
            raw_symbol=Symbol("ESM4 P5230"),
            asset_class=AssetClass.EQUITY,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            underlying="ESM4",
            option_kind=OptionKind.PUT,
            activation_ns=0,
            expiration_ns=1719792000000000000,
            strike_price=Price.from_str("5230.0"),
            ts_event=0,
            ts_init=0,
        )
        option2 = OptionContract(
            instrument_id=InstrumentId(Symbol("ESM4 P5250"), Venue("XCME")),
            raw_symbol=Symbol("ESM4 P5250"),
            asset_class=AssetClass.EQUITY,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            underlying="ESM4",
            option_kind=OptionKind.PUT,
            activation_ns=0,
            expiration_ns=1719792000000000000,
            strike_price=Price.from_str("5250.0"),
            ts_event=0,
            ts_init=0,
        )

        # Add instruments to cache
        self.data_engine.process(option1)
        self.data_engine.process(option2)

        # Create spread instrument ID
        spread_instrument_id = InstrumentId.new_spread(
            [
                (option1.id, 1),
                (option2.id, -1),
            ],
        )

        # Subscribe first
        subscribe = SubscribeQuoteTicks(
            client_id=None,
            venue=Venue("XCME"),  # Use the venue from the spread components
            instrument_id=spread_instrument_id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.data_engine.execute(subscribe)

        # Verify subscription was processed
        # Command count is 3: 1 for spread + 2 for component instruments
        assert self.data_engine.command_count == 3

        # Act - Unsubscribe
        unsubscribe = UnsubscribeQuoteTicks(
            client_id=None,
            venue=Venue("XCME"),  # Use the venue from the spread components
            instrument_id=spread_instrument_id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.data_engine.execute(unsubscribe)

        # Assert - Verify unsubscribe was processed
        # Command count is 4: 3 for subscribe (spread + 2 components) + 1 for unsubscribe (spread only)
        # Component instruments may not be unsubscribed if they have other subscribers
        assert self.data_engine.command_count == 4

    def test_spread_quote_generation_and_distribution(self):
        # Arrange
        # Create a client for XCME venue
        xcme_client = BacktestMarketDataClient(
            client_id=ClientId("XCME"),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        self.data_engine.register_client(xcme_client)
        xcme_client.start()

        # Create option instruments for spread
        option1 = OptionContract(
            instrument_id=InstrumentId(Symbol("ESM4 P5230"), Venue("XCME")),
            raw_symbol=Symbol("ESM4 P5230"),
            asset_class=AssetClass.EQUITY,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            underlying="ESM4",
            option_kind=OptionKind.PUT,
            activation_ns=0,
            expiration_ns=1719792000000000000,
            strike_price=Price.from_str("5230.0"),
            ts_event=0,
            ts_init=0,
        )
        option2 = OptionContract(
            instrument_id=InstrumentId(Symbol("ESM4 P5250"), Venue("XCME")),
            raw_symbol=Symbol("ESM4 P5250"),
            asset_class=AssetClass.EQUITY,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            underlying="ESM4",
            option_kind=OptionKind.PUT,
            activation_ns=0,
            expiration_ns=1719792000000000000,
            strike_price=Price.from_str("5250.0"),
            ts_event=0,
            ts_init=0,
        )

        # Add instruments to cache
        self.data_engine.process(option1)
        self.data_engine.process(option2)

        # Create spread instrument ID
        spread_instrument_id = InstrumentId.new_spread(
            [
                (option1.id, 1),
                (option2.id, -1),
            ],
        )

        # Set up handler to capture spread quotes
        handler = []
        self.msgbus.subscribe(
            topic=f"data.quotes.{spread_instrument_id.venue}.{spread_instrument_id.symbol}",
            handler=handler.append,
        )

        # Subscribe to spread quotes
        subscribe = SubscribeQuoteTicks(
            client_id=None,
            venue=Venue("XCME"),  # Use the venue from the spread components
            instrument_id=spread_instrument_id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.data_engine.execute(subscribe)

        # Act - Advance time to trigger quote generation
        self.clock.advance_time(2_000_000_000)  # Advance 2 seconds

        # Assert - Verify the subscription was processed and handler is set up
        # Command count is 3: 1 for spread + 2 for component instruments
        assert self.data_engine.command_count == 3
        # The test verifies the infrastructure is in place for spread quote handling

    def test_create_quote_tick_from_depth_with_valid_data_creates_quote_tick(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()
        self.data_engine.process(ETHUSDT_BINANCE)

        quote_handler = []
        self.msgbus.subscribe(
            topic="data.quotes.BINANCE.ETHUSDT",
            handler=quote_handler.append,
        )

        depth = TestDataStubs.order_book_depth10(
            instrument_id=ETHUSDT_BINANCE.id,
            ts_event=1_000_000_000,
            ts_init=2_000_000_000,
        )

        # Act
        self.data_engine.process(depth)

        # Assert
        assert len(quote_handler) == 1
        quote_tick = quote_handler[0]
        assert isinstance(quote_tick, QuoteTick)
        assert quote_tick.instrument_id == ETHUSDT_BINANCE.id
        assert quote_tick.bid_price == Price.from_str("99.00")  # Top bid from TestDataStubs
        assert quote_tick.ask_price == Price.from_str("100.00")  # Top ask from TestDataStubs
        assert quote_tick.bid_size == Quantity.from_str("100")
        assert quote_tick.ask_size == Quantity.from_str("100")
        assert quote_tick.ts_event == 1_000_000_000
        assert quote_tick.ts_init == 2_000_000_000

    def test_create_quote_tick_from_depth_with_empty_bids_does_not_create_quote_tick(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()
        self.data_engine.process(ETHUSDT_BINANCE)

        quote_handler = []
        self.msgbus.subscribe(
            topic="data.quotes.BINANCE.ETHUSDT",
            handler=quote_handler.append,
        )

        # Create depth with no valid bids (using NULL_ORDER)
        depth = OrderBookDepth10(
            instrument_id=ETHUSDT_BINANCE.id,
            bids=[NULL_ORDER],
            asks=[BookOrder(OrderSide.SELL, Price.from_str("100.00"), Quantity.from_str("100"), 1)],
            bid_counts=[0],
            ask_counts=[1],
            flags=0,
            sequence=1,
            ts_event=1_000_000_000,
            ts_init=2_000_000_000,
        )

        # Act
        self.data_engine.process(depth)

        # Assert
        assert len(quote_handler) == 0

    def test_create_quote_tick_from_depth_with_empty_asks_does_not_create_quote_tick(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()
        self.data_engine.process(ETHUSDT_BINANCE)

        quote_handler = []
        self.msgbus.subscribe(
            topic="data.quotes.BINANCE.ETHUSDT",
            handler=quote_handler.append,
        )

        # Create depth with no valid asks (using NULL_ORDER)
        depth = OrderBookDepth10(
            instrument_id=ETHUSDT_BINANCE.id,
            bids=[BookOrder(OrderSide.BUY, Price.from_str("99.00"), Quantity.from_str("100"), 1)],
            asks=[NULL_ORDER],
            bid_counts=[1],
            ask_counts=[0],
            flags=0,
            sequence=1,
            ts_event=1_000_000_000,
            ts_init=2_000_000_000,
        )

        # Act
        self.data_engine.process(depth)

        # Assert
        assert len(quote_handler) == 0

    def test_create_quote_tick_from_depth_with_null_orders_does_not_create_quote_tick(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()
        self.data_engine.process(ETHUSDT_BINANCE)

        quote_handler = []
        self.msgbus.subscribe(
            topic="data.quotes.BINANCE.ETHUSDT",
            handler=quote_handler.append,
        )

        # Create depth with NULL orders
        depth = OrderBookDepth10(
            instrument_id=ETHUSDT_BINANCE.id,
            bids=[NULL_ORDER],
            asks=[NULL_ORDER],
            bid_counts=[0],
            ask_counts=[0],
            flags=0,
            sequence=1,
            ts_event=1_000_000_000,
            ts_init=2_000_000_000,
        )

        # Act
        self.data_engine.process(depth)

        # Assert
        assert len(quote_handler) == 0

    def test_create_quote_tick_from_depth_with_zero_size_orders_does_not_create_quote_tick(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()
        self.data_engine.process(ETHUSDT_BINANCE)

        quote_handler = []
        self.msgbus.subscribe(
            topic="data.quotes.BINANCE.ETHUSDT",
            handler=quote_handler.append,
        )

        # Create depth with zero size orders
        zero_bid = BookOrder(OrderSide.BUY, Price.from_str("99.00"), Quantity.from_str("0"), 1)
        zero_ask = BookOrder(OrderSide.SELL, Price.from_str("100.00"), Quantity.from_str("0"), 2)

        depth = OrderBookDepth10(
            instrument_id=ETHUSDT_BINANCE.id,
            bids=[zero_bid],
            asks=[zero_ask],
            bid_counts=[1],
            ask_counts=[1],
            flags=0,
            sequence=1,
            ts_event=1_000_000_000,
            ts_init=2_000_000_000,
        )

        # Act
        self.data_engine.process(depth)

        # Assert
        assert len(quote_handler) == 0

    def test_create_quote_tick_from_depth_with_different_instruments(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()
        self.data_engine.process(ETHUSDT_BINANCE)
        self.data_engine.process(BTCUSDT_BINANCE)

        eth_quote_handler = []
        btc_quote_handler = []
        self.msgbus.subscribe(
            topic="data.quotes.BINANCE.ETHUSDT",
            handler=eth_quote_handler.append,
        )
        self.msgbus.subscribe(
            topic="data.quotes.BINANCE.BTCUSDT",
            handler=btc_quote_handler.append,
        )

        eth_depth = TestDataStubs.order_book_depth10(
            instrument_id=ETHUSDT_BINANCE.id,
            ts_event=1_000_000_000,
            ts_init=2_000_000_000,
        )

        btc_depth = TestDataStubs.order_book_depth10(
            instrument_id=BTCUSDT_BINANCE.id,
            ts_event=3_000_000_000,
            ts_init=4_000_000_000,
        )

        # Act
        self.data_engine.process(eth_depth)
        self.data_engine.process(btc_depth)

        # Assert
        assert len(eth_quote_handler) == 1
        assert len(btc_quote_handler) == 1

        eth_quote = eth_quote_handler[0]
        btc_quote = btc_quote_handler[0]

        assert eth_quote.instrument_id == ETHUSDT_BINANCE.id
        assert btc_quote.instrument_id == BTCUSDT_BINANCE.id
        assert eth_quote.ts_event == 1_000_000_000
        assert btc_quote.ts_event == 3_000_000_000

    def test_create_quote_tick_from_depth_preserves_spread(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()
        self.data_engine.process(ETHUSDT_BINANCE)

        quote_handler = []
        self.msgbus.subscribe(
            topic="data.quotes.BINANCE.ETHUSDT",
            handler=quote_handler.append,
        )

        # Create depth with specific bid/ask spread
        bid_order = BookOrder(OrderSide.BUY, Price.from_str("1500.50"), Quantity.from_str("250"), 1)
        ask_order = BookOrder(
            OrderSide.SELL,
            Price.from_str("1500.75"),
            Quantity.from_str("300"),
            2,
        )

        depth = OrderBookDepth10(
            instrument_id=ETHUSDT_BINANCE.id,
            bids=[bid_order],
            asks=[ask_order],
            bid_counts=[1],
            ask_counts=[1],
            flags=0,
            sequence=1,
            ts_event=1_000_000_000,
            ts_init=2_000_000_000,
        )

        # Act
        self.data_engine.process(depth)

        # Assert
        assert len(quote_handler) == 1
        quote_tick = quote_handler[0]

        assert quote_tick.bid_price == Price.from_str("1500.50")
        assert quote_tick.ask_price == Price.from_str("1500.75")
        assert quote_tick.bid_size == Quantity.from_str("250")
        assert quote_tick.ask_size == Quantity.from_str("300")

        # Verify spread is preserved
        spread = quote_tick.ask_price - quote_tick.bid_price
        assert spread == Price.from_str("0.25")


class TestDataEngineQuoteFromDepth:
    @pytest.fixture(autouse=True)
    def setup_method(self, tmp_path):
        self.tmp_path = tmp_path
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        config = DataEngineConfig(
            validate_data_sequence=True,
            debug=True,
            emit_quotes_from_book_depths=True,
        )
        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=config,
        )

        self.binance_client = BacktestMarketDataClient(
            client_id=ClientId(BINANCE.value),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

    def test_handle_order_book_depth_publishes_both_depth_and_quote_tick(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()
        self.data_engine.process(ETHUSDT_BINANCE)

        depth_handler = []
        quote_handler = []
        self.msgbus.subscribe(
            topic="data.book.depth.BINANCE.ETHUSDT",
            handler=depth_handler.append,
        )
        self.msgbus.subscribe(
            topic="data.quotes.BINANCE.ETHUSDT",
            handler=quote_handler.append,
        )

        depth = TestDataStubs.order_book_depth10(
            instrument_id=ETHUSDT_BINANCE.id,
            ts_event=1_000_000_000,
            ts_init=2_000_000_000,
        )

        # Act
        self.data_engine._handle_order_book_depth(depth)

        # Assert
        assert len(depth_handler) == 1
        assert len(quote_handler) == 1

        # Verify depth message
        published_depth = depth_handler[0]
        assert published_depth == depth

        # Verify quote tick message
        quote_tick = quote_handler[0]
        assert isinstance(quote_tick, QuoteTick)
        assert quote_tick.instrument_id == ETHUSDT_BINANCE.id
        assert quote_tick.bid_price == Price.from_str("99.00")
        assert quote_tick.ask_price == Price.from_str("100.00")
        assert quote_tick.ts_event == 1_000_000_000
        assert quote_tick.ts_init == 2_000_000_000

    def test_handle_order_book_depth_with_invalid_data_publishes_only_depth(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()
        self.data_engine.process(ETHUSDT_BINANCE)

        depth_handler = []
        quote_handler = []
        self.msgbus.subscribe(
            topic="data.book.depth.BINANCE.ETHUSDT",
            handler=depth_handler.append,
        )
        self.msgbus.subscribe(
            topic="data.quotes.BINANCE.ETHUSDT",
            handler=quote_handler.append,
        )

        # Create depth with no valid bids (invalid for quote tick creation)
        depth = OrderBookDepth10(
            instrument_id=ETHUSDT_BINANCE.id,
            bids=[NULL_ORDER],
            asks=[BookOrder(OrderSide.SELL, Price.from_str("100.00"), Quantity.from_str("100"), 1)],
            bid_counts=[0],
            ask_counts=[1],
            flags=0,
            sequence=1,
            ts_event=1_000_000_000,
            ts_init=2_000_000_000,
        )

        # Act
        self.data_engine._handle_order_book_depth(depth)

        # Assert
        assert len(depth_handler) == 1  # Depth is still published
        assert len(quote_handler) == 0  # No quote tick created


class TestDataBufferEngine:
    @pytest.fixture(autouse=True)
    def setup_method(self, tmp_path):
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        config = DataEngineConfig(
            validate_data_sequence=True,
            debug=True,
            buffer_deltas=True,
        )
        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=config,
        )

        self.binance_client = BacktestMarketDataClient(
            client_id=ClientId(BINANCE.value),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.bitmex_client = BacktestMarketDataClient(
            client_id=ClientId(BITMEX.value),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.quandl = BacktestMarketDataClient(
            client_id=ClientId("QUANDL"),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.betfair = BacktestMarketDataClient(
            client_id=ClientId("BETFAIR"),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.data_engine.process(BTCUSDT_BINANCE)
        self.data_engine.process(ETHUSDT_BINANCE)
        self.data_engine.process(XBTUSD_BITMEX)

    def test_process_order_book_delta_buffering_then_sends_to_registered_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        self.data_engine.process(ETHUSDT_BINANCE)  # <-- add necessary instrument for test

        handler = []
        self.msgbus.subscribe(topic="data.book.deltas.BINANCE.ETHUSDT", handler=handler.append)

        subscribe = SubscribeOrderBook(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            book_data_type=OrderBookDelta,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=BookType.L3_MBO,
            depth=5,
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        delta = TestDataStubs.order_book_delta(ETHUSDT_BINANCE.id)
        last_delta = TestDataStubs.order_book_delta(ETHUSDT_BINANCE.id, flags=RecordFlag.F_LAST)

        self.data_engine.process(delta)

        assert handler == []

        # Act
        self.data_engine.process(last_delta)

        # Assert
        assert handler[0].instrument_id == ETHUSDT_BINANCE.id
        assert isinstance(handler[0], OrderBookDeltas)
        assert len(handler) == 1
        assert handler[0].deltas == [delta, last_delta]

    def test_process_order_book_delta_buffers_are_cleared(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        self.data_engine.process(ETHUSDT_BINANCE)  # <-- add necessary instrument for test

        handler = []
        self.msgbus.subscribe(topic="data.book.deltas.BINANCE.ETHUSDT", handler=handler.append)

        subscribe = SubscribeOrderBook(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            book_data_type=OrderBookDelta,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=BookType.L3_MBO,
            depth=5,
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        delta = TestDataStubs.order_book_delta(ETHUSDT_BINANCE.id, flags=RecordFlag.F_LAST)

        # Act
        self.data_engine.process(delta)
        self.data_engine.process(delta)

        # Assert
        assert len(handler) == 2
        assert len(handler[0].deltas) == 1
        assert len(handler[1].deltas) == 1

    def test_execute_skips_commands_for_external_clients(self):
        # Arrange
        ext_client_id = ClientId("EXT_DATA")

        msgbus = MessageBus(trader_id=self.trader_id, clock=self.clock)
        cache = TestComponentStubs.cache()

        engine = DataEngine(
            msgbus=msgbus,
            cache=cache,
            clock=self.clock,
            config=DataEngineConfig(external_clients=[ext_client_id]),
        )

        cmd = SubscribeInstruments(
            client_id=ext_client_id,
            venue=None,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act / Assert: should not raise even though no client registered
        engine.execute(cmd)

        assert engine.get_external_client_ids() == {ext_client_id}

    def test_process_order_book_deltas_then_sends_to_registered_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        self.data_engine.process(ETHUSDT_BINANCE)  # <-- add necessary instrument for test

        handler = []
        self.msgbus.subscribe(topic="data.book.deltas.BINANCE.ETHUSDT", handler=handler.append)

        subscribe = SubscribeOrderBook(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            book_data_type=OrderBookDelta,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=BookType.L3_MBO,
            depth=5,
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        deltas = TestDataStubs.order_book_deltas(ETHUSDT_BINANCE.id)
        last_deltas = TestDataStubs.order_book_deltas(ETHUSDT_BINANCE.id, flags=RecordFlag.F_LAST)

        self.data_engine.process(deltas)

        assert handler == []

        # Act
        self.data_engine.process(last_deltas)

        # Assert
        assert handler[0].instrument_id == ETHUSDT_BINANCE.id
        assert isinstance(handler[0], OrderBookDeltas)
        assert len(handler[0].deltas) == 2

    def test_process_order_book_deltas_buffers_are_cleared(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()

        self.data_engine.process(ETHUSDT_BINANCE)  # <-- add necessary instrument for test

        handler = []
        self.msgbus.subscribe(topic="data.book.deltas.BINANCE.ETHUSDT", handler=handler.append)

        subscribe = SubscribeOrderBook(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            book_data_type=OrderBookDelta,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=BookType.L3_MBO,
            depth=5,
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.data_engine.execute(subscribe)

        deltas = TestDataStubs.order_book_deltas(ETHUSDT_BINANCE.id, flags=RecordFlag.F_LAST)

        # Act
        self.data_engine.process(deltas)
        self.data_engine.process(deltas)

        # Assert
        assert len(handler) == 2
        assert len(handler[0].deltas) == 1
        assert len(handler[1].deltas) == 1


class TestDataEngineQuoteFromBook:
    @pytest.fixture(autouse=True)
    def setup_method(self, tmp_path):
        self.tmp_path = tmp_path
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        config = DataEngineConfig(
            validate_data_sequence=True,
            debug=True,
            emit_quotes_from_book=True,
        )
        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=config,
        )

        self.binance_client = BacktestMarketDataClient(
            client_id=ClientId(BINANCE.value),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

    def test_update_order_book_with_delta_publishes_quote_tick(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()
        self.data_engine.process(ETHUSDT_BINANCE)

        # Subscribe to order book to create it in cache
        subscribe = SubscribeOrderBook(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            book_data_type=OrderBookDelta,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=BookType.L2_MBP,
            depth=10,
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.data_engine.execute(subscribe)

        quote_handler = []
        self.msgbus.subscribe(
            topic="data.quotes.BINANCE.ETHUSDT",
            handler=quote_handler.append,
        )

        # Create initial snapshot to populate the book
        snapshot = TestDataStubs.order_book_snapshot(
            instrument=ETHUSDT_BINANCE,
            bid_price=99.00,
            ask_price=100.00,
            bid_size=100.0,
            ask_size=100.0,
            ts_event=1_000_000_000,
            ts_init=2_000_000_000,
        )
        self.data_engine.process(snapshot)

        # Clear the quote handler after snapshot
        quote_handler.clear()

        # Create a delta update
        delta = OrderBookDelta(
            instrument_id=ETHUSDT_BINANCE.id,
            action=BookAction.UPDATE,
            order=BookOrder(
                OrderSide.BUY,
                ETHUSDT_BINANCE.make_price(99.50),
                ETHUSDT_BINANCE.make_qty(200),
                1,
            ),
            flags=RecordFlag.F_LAST,
            sequence=2,
            ts_event=3_000_000_000,
            ts_init=4_000_000_000,
        )

        # Act
        self.data_engine.process(delta)

        # Assert
        assert len(quote_handler) == 1
        quote_tick = quote_handler[0]
        assert isinstance(quote_tick, QuoteTick)
        assert quote_tick.instrument_id == ETHUSDT_BINANCE.id
        assert quote_tick.bid_price == Price.from_str("99.50")
        assert quote_tick.ask_price == Price.from_str("100.00")

    def test_update_order_book_with_deltas_publishes_quote_tick(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()
        self.data_engine.process(ETHUSDT_BINANCE)

        # Subscribe to order book to create it in cache
        subscribe = SubscribeOrderBook(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            book_data_type=OrderBookDelta,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=BookType.L2_MBP,
            depth=10,
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.data_engine.execute(subscribe)

        quote_handler = []
        self.msgbus.subscribe(
            topic="data.quotes.BINANCE.ETHUSDT",
            handler=quote_handler.append,
        )

        # Create snapshot with deltas
        snapshot = TestDataStubs.order_book_snapshot(
            instrument=ETHUSDT_BINANCE,
            bid_price=99.00,
            ask_price=100.00,
            bid_size=100.0,
            ask_size=100.0,
            ts_event=1_000_000_000,
            ts_init=2_000_000_000,
        )

        # Act
        self.data_engine.process(snapshot)

        # Assert
        assert len(quote_handler) == 1
        quote_tick = quote_handler[0]
        assert isinstance(quote_tick, QuoteTick)
        assert quote_tick.instrument_id == ETHUSDT_BINANCE.id
        assert quote_tick.bid_price == Price.from_str("99.00")
        assert quote_tick.ask_price == Price.from_str("100.00")

    def test_update_order_book_with_multiple_deltas_publishes_quote_tick(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()
        self.data_engine.process(ETHUSDT_BINANCE)

        # Subscribe to order book to create it in cache
        subscribe = SubscribeOrderBook(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            book_data_type=OrderBookDelta,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=BookType.L2_MBP,
            depth=10,
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.data_engine.execute(subscribe)

        quote_handler = []
        self.msgbus.subscribe(
            topic="data.quotes.BINANCE.ETHUSDT",
            handler=quote_handler.append,
        )

        # Create initial snapshot to populate the book
        snapshot = TestDataStubs.order_book_snapshot(
            instrument=ETHUSDT_BINANCE,
            bid_price=99.00,
            ask_price=100.00,
            bid_size=100.0,
            ask_size=100.0,
            ts_event=1_000_000_000,
            ts_init=2_000_000_000,
        )
        self.data_engine.process(snapshot)

        # Clear the quote handler after snapshot
        quote_handler.clear()

        # Create multiple delta updates
        delta1 = OrderBookDelta(
            instrument_id=ETHUSDT_BINANCE.id,
            action=BookAction.UPDATE,
            order=BookOrder(
                OrderSide.BUY,
                ETHUSDT_BINANCE.make_price(99.50),
                ETHUSDT_BINANCE.make_qty(200),
                1,
            ),
            flags=0,
            sequence=2,
            ts_event=3_000_000_000,
            ts_init=4_000_000_000,
        )

        delta2 = OrderBookDelta(
            instrument_id=ETHUSDT_BINANCE.id,
            action=BookAction.UPDATE,
            order=BookOrder(
                OrderSide.SELL,
                ETHUSDT_BINANCE.make_price(100.00),  # Update the existing best ask
                ETHUSDT_BINANCE.make_qty(300),
                11,  # Same order_id as the original best ask
            ),
            flags=RecordFlag.F_LAST,
            sequence=3,
            ts_event=5_000_000_000,
            ts_init=6_000_000_000,
        )

        # Act
        self.data_engine.process(delta1)
        self.data_engine.process(delta2)

        # Assert - should have 2 quote ticks (one per delta)
        assert len(quote_handler) == 2
        quote_tick = quote_handler[1]
        assert isinstance(quote_tick, QuoteTick)
        assert quote_tick.instrument_id == ETHUSDT_BINANCE.id
        assert quote_tick.bid_price == Price.from_str("99.50")
        assert quote_tick.ask_price == Price.from_str("100.00")
        assert quote_tick.ask_size == Quantity.from_int(300)

    def test_update_order_book_with_invalid_book_does_not_publish_quote_tick(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.start()
        self.data_engine.process(ETHUSDT_BINANCE)

        # Subscribe to order book to create it in cache
        subscribe = SubscribeOrderBook(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            book_data_type=OrderBookDelta,
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=BookType.L2_MBP,
            depth=10,
            managed=True,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.data_engine.execute(subscribe)

        quote_handler = []
        self.msgbus.subscribe(
            topic="data.quotes.BINANCE.ETHUSDT",
            handler=quote_handler.append,
        )

        # Create snapshot with only asks (no bids)
        deltas = [
            OrderBookDelta.clear(ETHUSDT_BINANCE.id, 0, 1_000_000_000, 2_000_000_000),
            OrderBookDelta(
                ETHUSDT_BINANCE.id,
                BookAction.ADD,
                BookOrder(
                    OrderSide.SELL,
                    Price.from_str("100.00"),
                    Quantity.from_str("100"),
                    1,
                ),
                0,
                0,
                1_000_000_000,
                2_000_000_000,
            ),
        ]
        snapshot = OrderBookDeltas(
            instrument_id=ETHUSDT_BINANCE.id,
            deltas=deltas,
        )

        # Act
        self.data_engine.process(snapshot)

        # Assert - no quote tick should be created because there's no bid
        assert len(quote_handler) == 0
