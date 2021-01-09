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

from nautilus_trader.backtest.data_client import BacktestDataClient
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import TestLogger
from nautilus_trader.common.messages import DataRequest
from nautilus_trader.common.messages import DataResponse
from nautilus_trader.common.messages import Subscribe
from nautilus_trader.common.messages import Unsubscribe
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.core.fsm import InvalidStateTrigger
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.model.bar import Bar
from nautilus_trader.model.bar import BarData
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.bar import BarType
from nautilus_trader.model.commands import VenueCommand
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeMatchId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instrument import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.tick import TradeTick
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.mocks import ObjectStorer
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH


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

        self.portfolio.register_cache(self.data_engine.cache)

        self.binance_client = BacktestDataClient(
            instruments=[BTCUSDT_BINANCE, ETHUSDT_BINANCE],
            venue=BINANCE,
            engine=self.data_engine,
            clock=self.clock,
            logger=self.logger,
        )

        self.bitmex_client = BacktestDataClient(
            instruments=[XBTUSD_BITMEX],
            venue=BITMEX,
            engine=self.data_engine,
            clock=self.clock,
            logger=self.logger,
        )

    def test_registered_venues(self):
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

    def test_register_client_successfully_adds_client(self):
        # Arrange
        # Act
        self.data_engine.register_client(self.binance_client)

        # Assert
        self.assertIn(BINANCE, self.data_engine.registered_venues)

    def test_deregister_client_successfully_removes_client(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        # Act
        self.data_engine.deregister_client(self.binance_client)

        # Assert
        self.assertNotIn(BINANCE, self.data_engine.registered_venues)

    def test_register_strategy_successfully_registered_with_strategy(self):
        # Arrange
        strategy = TradingStrategy("000")

        # Act
        strategy.register_data_engine(self.data_engine)

        # Assert
        self.assertEqual(self.data_engine.cache, strategy.data)

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

        command = VenueCommand(
            venue=BINANCE,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        # Act
        self.data_engine.execute(command)

        # Assert
        self.assertEqual(1, self.data_engine.command_count)

    def test_send_request_when_no_data_clients_registered_does_nothing(self):
        # Arrange
        handler = []
        request = DataRequest(
            venue=Venue("RANDOM"),
            data_type=QuoteTick,
            metadata={
                "Symbol": Symbol("SOMETHING", Venue("RANDOM")),
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

    def test_send_data_request_when_data_type_unrecognized_logs_and_does_nothing(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        handler = []
        request = DataRequest(
            venue=BINANCE,
            data_type=str,  # str data type is invalid
            metadata={
                "Symbol": Symbol("SOMETHING", Venue("RANDOM")),
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

    def test_send_data_request_with_duplicate_ids_logs_and_does_not_handle_second(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        handler = []
        uuid = self.uuid_factory.generate()  # We'll use this as a duplicate

        request1 = DataRequest(
            venue=BINANCE,
            data_type=QuoteTick,  # str data type is invalid
            metadata={
                "Symbol": Symbol("SOMETHING", Venue("RANDOM")),
                "FromDateTime": None,
                "ToDateTime": None,
                "Limit": 1000,
            },
            callback=handler.append,
            request_id=uuid,  # Duplicate
            request_timestamp=self.clock.utc_now(),
        )

        request2 = DataRequest(
            venue=BINANCE,
            data_type=QuoteTick,  # str data type is invalid
            metadata={
                "Symbol": Symbol("SOMETHING", Venue("RANDOM")),
                "FromDateTime": None,
                "ToDateTime": None,
                "Limit": 1000,
            },
            callback=handler.append,
            request_id=uuid,  # Duplicate
            request_timestamp=self.clock.utc_now(),
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
            venue=BINANCE,
            data_type=str,  # str data type is invalid
            metadata={},  # Invalid anyway
            handler=[].append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
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
            venue=BINANCE,
            data_type=QuoteTick,
            metadata={"Symbol": ETHUSDT_BINANCE.symbol},
            handler=[].append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        # Act
        self.data_engine.execute(subscribe)
        self.data_engine.execute(subscribe)

        # Assert
        self.assertEqual(2, self.data_engine.command_count)

    def test_execute_unsubscribe_when_data_type_unrecognized_logs_and_does_nothing(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        handler = []
        unsubscribe = Unsubscribe(
            venue=BINANCE,
            data_type=str,  # str data type is invalid
            metadata={},  # Invalid anyway
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
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
            venue=BINANCE,
            data_type=QuoteTick,
            metadata={"Symbol": ETHUSDT_BINANCE.symbol},
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        # Act
        self.data_engine.execute(unsubscribe)

        # Assert
        self.assertEqual(1, self.data_engine.command_count)

    def test_receive_response_when_no_data_clients_registered_does_nothing(self):
        # Arrange
        response = DataResponse(
            venue=BINANCE,
            data_type=QuoteTick,
            metadata={},
            data=[],
            correlation_id=self.uuid_factory.generate(),
            response_id=self.uuid_factory.generate(),
            response_timestamp=self.clock.utc_now(),
        )

        # Act
        self.data_engine.receive(response)

        # Assert
        self.assertEqual(1, self.data_engine.response_count)

    def test_update_instruments_sends_request_to_self(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)

        # Act
        self.data_engine.update_instruments(BINANCE)

        # Assert
        self.assertEqual(1, self.data_engine.request_count)
        self.assertEqual(0, self.data_engine.data_count)

    def test_update_instruments_all_sends_request_to_self(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.data_engine.register_client(self.bitmex_client)

        # Act
        self.data_engine.update_instruments_all()

        # Assert
        self.assertEqual(2, self.data_engine.request_count)
        self.assertEqual(0, self.data_engine.data_count)

    def test_process_unrecognized_data_type_logs_and_does_nothing(self):
        # Arrange
        # Act
        self.data_engine.process("DATA!")  # Invalid

        # Assert
        self.assertEqual(1, self.data_engine.data_count)

    def test_process_data_places_data_on_queue(self):
        # Arrange
        tick = TestStubs.trade_tick_5decimal()

        # Act
        self.data_engine.process(tick)

        # Assert
        self.assertEqual(1, self.data_engine.data_count)

    def test_execute_subscribe_instrument_then_adds_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        subscribe = Subscribe(
            venue=BINANCE,
            data_type=Instrument,
            metadata={"Symbol": ETHUSDT_BINANCE.symbol},
            handler=[].append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        self.assertEqual([ETHUSDT_BINANCE.symbol], self.data_engine.subscribed_instruments)

    def test_execute_unsubscribe_instrument_then_removes_handler(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler = []
        subscribe = Subscribe(
            venue=BINANCE,
            data_type=Instrument,
            metadata={"Symbol": ETHUSDT_BINANCE.symbol},
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        self.data_engine.execute(subscribe)

        unsubscribe = Unsubscribe(
            venue=BINANCE,
            data_type=Instrument,
            metadata={"Symbol": ETHUSDT_BINANCE.symbol},
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
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
            venue=BINANCE,
            data_type=Instrument,
            metadata={"Symbol": ETHUSDT_BINANCE.symbol},
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        self.data_engine.execute(subscribe)

        # Act
        self.data_engine.process(ETHUSDT_BINANCE)

        # Assert
        self.assertEqual([ETHUSDT_BINANCE], handler)

    def test_process_instrument_when_subscribers_then_sends_to_registered_handlers(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler1 = []
        subscribe1 = Subscribe(
            venue=BINANCE,
            data_type=Instrument,
            metadata={"Symbol": ETHUSDT_BINANCE.symbol},
            handler=handler1.append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        handler2 = []
        subscribe2 = Subscribe(
            venue=BINANCE,
            data_type=Instrument,
            metadata={"Symbol": ETHUSDT_BINANCE.symbol},
            handler=handler2.append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        self.data_engine.execute(subscribe1)
        self.data_engine.execute(subscribe2)

        # Act
        self.data_engine.process(ETHUSDT_BINANCE)

        # Assert
        self.assertEqual([ETHUSDT_BINANCE.symbol], self.data_engine.subscribed_instruments)
        self.assertEqual([ETHUSDT_BINANCE], handler1)
        self.assertEqual([ETHUSDT_BINANCE], handler2)

    def test_execute_subscribe_for_quote_ticks(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler = []
        subscribe = Subscribe(
            venue=BINANCE,
            data_type=QuoteTick,
            metadata={"Symbol": ETHUSDT_BINANCE.symbol},
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        self.data_engine.execute(subscribe)

        # Assert
        self.assertEqual([ETHUSDT_BINANCE.symbol], self.data_engine.subscribed_quote_ticks)

    def test_execute_unsubscribe_for_quote_ticks(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler = []
        subscribe = Subscribe(
            venue=BINANCE,
            data_type=QuoteTick,
            metadata={"Symbol": ETHUSDT_BINANCE.symbol},
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        self.data_engine.execute(subscribe)

        unsubscribe = Unsubscribe(
            venue=BINANCE,
            data_type=QuoteTick,
            metadata={"Symbol": ETHUSDT_BINANCE.symbol},
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
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
            venue=BINANCE,
            data_type=QuoteTick,
            metadata={"Symbol": ETHUSDT_BINANCE.symbol},
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        self.data_engine.execute(subscribe)

        tick = QuoteTick(
            ETHUSDT_BINANCE.symbol,
            Price("100.003"),
            Price("100.003"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        self.assertEqual([ETHUSDT_BINANCE.symbol], self.data_engine.subscribed_quote_ticks)
        self.assertEqual([tick], handler)

    def test_process_quote_tick_when_subscribers_then_sends_to_registered_handlers(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler1 = []
        subscribe1 = Subscribe(
            venue=BINANCE,
            data_type=QuoteTick,
            metadata={"Symbol": ETHUSDT_BINANCE.symbol},
            handler=handler1.append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        handler2 = []
        subscribe2 = Subscribe(
            venue=BINANCE,
            data_type=QuoteTick,
            metadata={"Symbol": ETHUSDT_BINANCE.symbol},
            handler=handler2.append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        self.data_engine.execute(subscribe1)
        self.data_engine.execute(subscribe2)

        tick = QuoteTick(
            ETHUSDT_BINANCE.symbol,
            Price("100.003"),
            Price("100.003"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        self.assertEqual([ETHUSDT_BINANCE.symbol], self.data_engine.subscribed_quote_ticks)
        self.assertEqual([tick], handler1)
        self.assertEqual([tick], handler2)

    def test_subscribe_trade_tick_then_subscribes(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler = []
        subscribe = Subscribe(
            venue=BINANCE,
            data_type=TradeTick,
            metadata={"Symbol": ETHUSDT_BINANCE.symbol},
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        # Act
        self.data_engine.execute(subscribe)

        # Assert
        self.assertEqual([ETHUSDT_BINANCE.symbol], self.data_engine.subscribed_trade_ticks)

    def test_unsubscribe_trade_tick_then_unsubscribes(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler = []
        subscribe = Subscribe(
            venue=BINANCE,
            data_type=TradeTick,
            metadata={"Symbol": ETHUSDT_BINANCE.symbol},
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        self.data_engine.execute(subscribe)

        unsubscribe = Unsubscribe(
            venue=BINANCE,
            data_type=TradeTick,
            metadata={"Symbol": ETHUSDT_BINANCE.symbol},
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
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
            venue=BINANCE,
            data_type=TradeTick,
            metadata={"Symbol": ETHUSDT_BINANCE.symbol},
            handler=handler.append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        self.data_engine.execute(subscribe)

        tick = TradeTick(
            ETHUSDT_BINANCE.symbol,
            Price("1050.00000"),
            Quantity(100),
            OrderSide.BUY,
            TradeMatchId("123456789"),
            UNIX_EPOCH,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        self.assertEqual([tick], handler)

    def test_process_trade_tick_when_subscribers_then_sends_to_registered_handlers(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        handler1 = []
        subscribe1 = Subscribe(
            venue=BINANCE,
            data_type=TradeTick,
            metadata={"Symbol": ETHUSDT_BINANCE.symbol},
            handler=handler1.append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        handler2 = []
        subscribe2 = Subscribe(
            venue=BINANCE,
            data_type=TradeTick,
            metadata={"Symbol": ETHUSDT_BINANCE.symbol},
            handler=handler2.append,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        self.data_engine.execute(subscribe1)
        self.data_engine.execute(subscribe2)

        tick = TradeTick(
            ETHUSDT_BINANCE.symbol,
            Price("1050.00000"),
            Quantity(100),
            OrderSide.BUY,
            TradeMatchId("123456789"),
            UNIX_EPOCH,
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
        bar_type = BarType(ETHUSDT_BINANCE.symbol, bar_spec, is_internal_aggregation=True)

        handler = ObjectStorer()
        subscribe = Subscribe(
            venue=BINANCE,
            data_type=Bar,
            metadata={"BarType": bar_type},
            handler=handler.store_2,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
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
        bar_type = BarType(ETHUSDT_BINANCE.symbol, bar_spec, is_internal_aggregation=True)

        handler = ObjectStorer()
        subscribe = Subscribe(
            venue=BINANCE,
            data_type=Bar,
            metadata={"BarType": bar_type},
            handler=handler.store_2,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        self.data_engine.execute(subscribe)

        unsubscribe = Unsubscribe(
            venue=BINANCE,
            data_type=Bar,
            metadata={"BarType": bar_type},
            handler=handler.store_2,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
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
        bar_type = BarType(ETHUSDT_BINANCE.symbol, bar_spec, is_internal_aggregation=True)

        handler = ObjectStorer()
        subscribe = Subscribe(
            venue=BINANCE,
            data_type=Bar,
            metadata={"BarType": bar_type},
            handler=handler.store_2,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        self.data_engine.execute(subscribe)

        bar = Bar(
            Price("1051.00000"),
            Price("1055.00000"),
            Price("1050.00000"),
            Price("1052.00000"),
            Quantity(100),
            UNIX_EPOCH,
        )

        data = BarData(bar_type, bar)

        # Act
        self.data_engine.process(data)

        # Assert
        self.assertEqual([(bar_type, bar)], handler.get_store())

    def test_process_bar_when_subscribers_then_sends_to_registered_handlers(self):
        # Arrange
        self.data_engine.register_client(self.binance_client)
        self.binance_client.connect()

        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(ETHUSDT_BINANCE.symbol, bar_spec, is_internal_aggregation=True)

        handler1 = ObjectStorer()
        subscribe1 = Subscribe(
            venue=BINANCE,
            data_type=Bar,
            metadata={"BarType": bar_type},
            handler=handler1.store_2,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        handler2 = ObjectStorer()
        subscribe2 = Subscribe(
            venue=BINANCE,
            data_type=Bar,
            metadata={"BarType": bar_type},
            handler=handler2.store_2,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        self.data_engine.execute(subscribe1)
        self.data_engine.execute(subscribe2)

        bar = Bar(
            Price("1051.00000"),
            Price("1055.00000"),
            Price("1050.00000"),
            Price("1052.00000"),
            Quantity(100),
            UNIX_EPOCH,
        )

        data = BarData(bar_type, bar)

        # Act
        self.data_engine.process(data)

        # Assert
        self.assertEqual([(bar_type, bar)], handler1.get_store())
        self.assertEqual([(bar_type, bar)], handler2.get_store())
