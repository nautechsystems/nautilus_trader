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


from nautilus_trader.backtest.data_client import BacktestMarketDataClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.option_contract import OptionContract
from nautilus_trader.model.instruments.option_spread import OptionSpread
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestBacktestMarketDataClient:
    def setup_method(self):
        # Setup test components
        self.clock = TestComponentStubs.clock()
        self.msgbus = MessageBus(
            trader_id=TestIdStubs.trader_id(),
            clock=self.clock,
        )
        self.cache = Cache()

        # Create test option instruments
        self.option1 = OptionContract(
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
            expiration_ns=1719792000000000000,  # 2024-06-30
            strike_price=Price.from_str("5230.0"),
            ts_event=0,
            ts_init=0,
        )
        self.option2 = OptionContract(
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
            expiration_ns=1719792000000000000,  # 2024-06-30
            strike_price=Price.from_str("5250.0"),
            ts_event=0,
            ts_init=0,
        )

        # Add instruments to cache
        self.cache.add_instrument(self.option1)
        self.cache.add_instrument(self.option2)

        # Create spread instrument ID
        self.spread_instrument_id = InstrumentId.new_spread(
            [
                (self.option1.id, 1),
                (self.option2.id, -1),
            ],
        )

        # Create BacktestMarketDataClient
        self.client = BacktestMarketDataClient(
            client_id=ClientId("BACKTEST"),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

    def test_initialization(self):
        # Arrange, Act, Assert
        assert self.client.id == ClientId("BACKTEST")
        assert self.client._cache == self.cache
        assert self.client._clock == self.clock

    def test_request_instrument_creates_spread_from_components(self):
        # Arrange - components are already in cache
        from nautilus_trader.data.messages import RequestInstrument

        request = RequestInstrument(
            instrument_id=self.spread_instrument_id,
            start=None,
            end=None,
            client_id=self.client.id,
            venue=None,
            callback=lambda x: None,  # Dummy callback
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        # Act
        self.client.request_instrument(request)

        # Assert - the spread instrument should now be in cache
        spread_instrument = self.cache.instrument(self.spread_instrument_id)
        assert spread_instrument is not None
        assert isinstance(spread_instrument, OptionSpread)
        assert spread_instrument.id == self.spread_instrument_id
        assert spread_instrument.underlying == ""  # Not necessary for option spreads
        assert spread_instrument.strategy_type == "SPREAD"  # Set by BacktestMarketDataClient
        assert spread_instrument.quote_currency == Currency.from_str("USD")
        assert spread_instrument.asset_class == self.option1.asset_class
        assert spread_instrument.price_precision == self.option1.price_precision

    def test_request_instrument_with_missing_component_logs_error(self):
        # Arrange - create a spread ID with a missing component
        missing_option_id = InstrumentId(Symbol("ESM4 P5270"), Venue("XCME"))
        spread_with_missing_component = InstrumentId.new_spread(
            [
                (self.option1.id, 1),
                (missing_option_id, -1),  # This option is not in cache
            ],
        )

        from nautilus_trader.data.messages import RequestInstrument

        request = RequestInstrument(
            instrument_id=spread_with_missing_component,
            start=None,
            end=None,
            client_id=self.client.id,
            venue=None,
            callback=lambda x: None,  # Dummy callback
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        # Act
        self.client.request_instrument(request)

        # Assert - the spread instrument should not be in cache
        spread_instrument = self.cache.instrument(spread_with_missing_component)
        assert spread_instrument is None

    def test_request_instrument_with_non_spread_instrument_works_normally(self):
        # Arrange - request a regular option instrument
        from nautilus_trader.data.messages import RequestInstrument

        request = RequestInstrument(
            instrument_id=self.option1.id,
            start=None,
            end=None,
            client_id=self.client.id,
            venue=None,
            callback=lambda x: None,  # Dummy callback
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        # Act
        self.client.request_instrument(request)

        # Assert - the option instrument should still be in cache (it was already there)
        instrument = self.cache.instrument(self.option1.id)
        assert instrument is not None
        assert instrument == self.option1

    def test_request_instrument_with_unknown_instrument_logs_error(self):
        # Arrange - request an unknown instrument
        unknown_id = InstrumentId(Symbol("UNKNOWN"), Venue("XCME"))
        from nautilus_trader.data.messages import RequestInstrument

        request = RequestInstrument(
            instrument_id=unknown_id,
            start=None,
            end=None,
            client_id=self.client.id,
            venue=None,
            callback=lambda x: None,  # Dummy callback
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        # Act
        self.client.request_instrument(request)

        # Assert - the unknown instrument should not be in cache
        instrument = self.cache.instrument(unknown_id)
        assert instrument is None

    def test_subscribe_order_book_depth_with_valid_instrument(self):
        # Arrange
        audusd = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        self.cache.add_instrument(audusd)

        command = SubscribeOrderBook(
            instrument_id=audusd.id,
            book_data_type=OrderBookDepth10,
            book_type=BookType.L2_MBP,
            client_id=ClientId("BACKTEST"),
            venue=audusd.id.venue,
            command_id=UUID4(),
            ts_init=0,
            depth=10,
        )

        # Act & Assert - should not raise an exception
        self.client.subscribe_order_book_depth(command)

    def test_subscribe_order_book_depth_with_missing_instrument_logs_error(self):
        # Arrange
        unknown_id = InstrumentId(Symbol("UNKNOWN"), Venue("SIM"))

        command = SubscribeOrderBook(
            instrument_id=unknown_id,
            book_data_type=OrderBookDepth10,
            book_type=BookType.L2_MBP,
            client_id=ClientId("BACKTEST"),
            venue=unknown_id.venue,
            command_id=UUID4(),
            ts_init=0,
            depth=10,
        )

        # Act & Assert - should not raise an exception (logs error instead)
        self.client.subscribe_order_book_depth(command)

    def test_unsubscribe_order_book_depth(self):
        # Arrange
        audusd = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        self.cache.add_instrument(audusd)

        # First subscribe
        subscribe_command = SubscribeOrderBook(
            instrument_id=audusd.id,
            book_data_type=OrderBookDepth10,
            book_type=BookType.L2_MBP,
            client_id=ClientId("BACKTEST"),
            venue=audusd.id.venue,
            command_id=UUID4(),
            ts_init=0,
            depth=10,
        )
        self.client.subscribe_order_book_depth(subscribe_command)

        # Create unsubscribe command
        unsubscribe_command = UnsubscribeOrderBook(
            instrument_id=audusd.id,
            book_data_type=OrderBookDepth10,
            client_id=ClientId("BACKTEST"),
            venue=audusd.id.venue,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act & Assert - should not raise an exception
        self.client.unsubscribe_order_book_depth(unsubscribe_command)
