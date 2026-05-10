# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from datetime import UTC
from datetime import datetime
from math import exp

from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.model.data import IndexPriceUpdate
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.greeks import GreeksCalculator
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.instruments import FuturesContract
from nautilus_trader.model.instruments import OptionContract
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestGreeksCalculator:
    def setup_method(self):
        # Setup test components
        self.clock = TestClock()
        # Set clock to 30 days before option expiry (2024-02-14 16:00 UTC)
        self.clock.set_time(1_707_926_400_000_000_000)
        self.cache = TestComponentStubs.cache()

        # Create test instruments
        self.underlying_id = InstrumentId(Symbol("AAPL"), Venue("XNAS"))
        self.option_id = InstrumentId(Symbol("AAPL240315C00150000"), Venue("XNAS"))

        # Create underlying equity
        self.underlying = Equity(
            instrument_id=self.underlying_id,
            raw_symbol=Symbol("AAPL"),
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            lot_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        # Create call option
        expiry_date = datetime(2024, 3, 15, 16, 0, 0, tzinfo=UTC)
        expiry_ns = int(expiry_date.timestamp() * 1_000_000_000)
        self.option = OptionContract(
            instrument_id=self.option_id,
            raw_symbol=Symbol("AAPL240315C00150000"),
            asset_class=AssetClass.EQUITY,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            underlying="AAPL",
            option_kind=OptionKind.CALL,
            activation_ns=0,
            expiration_ns=expiry_ns,
            strike_price=Price.from_str("150.00"),
            ts_event=0,
            ts_init=0,
        )

        # Add instruments to cache
        self.cache.add_instrument(self.underlying)
        self.cache.add_instrument(self.option)

        # Create GreeksCalculator
        self.greeks_calculator = GreeksCalculator(
            cache=self.cache,
            clock=self.clock,
        )

        # Create order factory for creating positions
        self.order_factory = OrderFactory(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=StrategyId("S-001"),
            clock=self.clock,
        )

    def test_instrument_greeks_without_cache(self):
        # Test basic greeks calculation without caching
        # Add prices
        underlying_price = Price.from_str("155.00")
        option_price = Price.from_str("8.50")

        quote_underlying = QuoteTick(
            instrument_id=self.underlying_id,
            bid_price=underlying_price,
            ask_price=underlying_price,
            bid_size=Quantity.from_int(100),
            ask_size=Quantity.from_int(100),
            ts_event=self.clock.timestamp_ns(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.cache.add_quote_tick(quote_underlying)

        quote_option = QuoteTick(
            instrument_id=self.option_id,
            bid_price=option_price,
            ask_price=option_price,
            bid_size=Quantity.from_int(100),
            ask_size=Quantity.from_int(100),
            ts_event=self.clock.timestamp_ns(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.cache.add_quote_tick(quote_option)

        # Calculate greeks
        greeks = self.greeks_calculator.instrument_greeks(
            instrument_id=self.option_id,
            cache_greeks=False,
            ts_event=self.clock.timestamp_ns(),
        )

        # Expected values for AAPL $150C with underlying at 155, 30 DTE, IV ~32.6%
        assert abs(greeks.price - 8.500007629394531) < 1e-3, (
            f"Price mismatch: {greeks.price} vs 8.500007629394531"
        )
        assert abs(greeks.vol - 0.3260962941) < 1e-5, f"Vol mismatch: {greeks.vol} vs 0.3260962941"
        assert abs(greeks.delta - 0.6522504091262817) < 1e-3, (
            f"Delta mismatch: {greeks.delta} vs 0.6522504091262817"
        )
        assert abs(greeks.gamma - 0.025358643383) < 1e-3, (
            f"Gamma mismatch: {greeks.gamma} vs 0.025358643383"
        )
        assert abs(greeks.vega - 0.163179779053) < 1e-3, (
            f"Vega mismatch: {greeks.vega} vs 0.163179779053"
        )
        assert abs(greeks.theta - (-0.087698151199)) < 1e-3, (
            f"Theta mismatch: {greeks.theta} vs -0.087698151199"
        )

    def test_instrument_greeks_with_caching(self):
        # Test greeks calculation with caching
        underlying_price = Price.from_str("155.00")
        option_price = Price.from_str("8.50")

        quote_underlying = QuoteTick(
            instrument_id=self.underlying_id,
            bid_price=underlying_price,
            ask_price=underlying_price,
            bid_size=Quantity.from_int(100),
            ask_size=Quantity.from_int(100),
            ts_event=self.clock.timestamp_ns(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.cache.add_quote_tick(quote_underlying)

        quote_option = QuoteTick(
            instrument_id=self.option_id,
            bid_price=option_price,
            ask_price=option_price,
            bid_size=Quantity.from_int(100),
            ask_size=Quantity.from_int(100),
            ts_event=self.clock.timestamp_ns(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.cache.add_quote_tick(quote_option)

        # Calculate and cache greeks
        greeks1 = self.greeks_calculator.instrument_greeks(
            instrument_id=self.option_id,
            cache_greeks=True,
            ts_event=self.clock.timestamp_ns(),
        )

        cached_vol = greeks1.vol

        # Use cached greeks
        greeks2 = self.greeks_calculator.instrument_greeks(
            instrument_id=self.option_id,
            use_cached_greeks=True,
            ts_event=self.clock.timestamp_ns(),
        )
        # Cached values should match exactly
        tol = 1e-10
        assert abs(greeks2.price - greeks1.price) < tol, (
            f"Cached price mismatch: {greeks2.price} vs {greeks1.price}"
        )
        assert abs(greeks2.vol - cached_vol) < tol, (
            f"Cached vol mismatch: {greeks2.vol} vs {cached_vol}"
        )
        assert abs(greeks2.delta - greeks1.delta) < tol, (
            f"Cached delta mismatch: {greeks2.delta} vs {greeks1.delta}"
        )
        assert abs(greeks2.gamma - greeks1.gamma) < tol, (
            f"Cached gamma mismatch: {greeks2.gamma} vs {greeks1.gamma}"
        )
        assert abs(greeks2.vega - greeks1.vega) < tol, (
            f"Cached vega mismatch: {greeks2.vega} vs {greeks1.vega}"
        )
        assert abs(greeks2.theta - greeks1.theta) < tol, (
            f"Cached theta mismatch: {greeks2.theta} vs {greeks1.theta}"
        )

    def test_instrument_greeks_returns_none_without_direct_or_cached_future_price(self):
        future_id = InstrumentId(Symbol("ESH4"), Venue("GLBX"))
        call_id = InstrumentId(Symbol("ESH4C150"), Venue("GLBX"))
        put_id = InstrumentId(Symbol("ESH4P150"), Venue("GLBX"))
        expiry_date = datetime(2024, 3, 15, 16, 0, 0, tzinfo=UTC)
        expiry_ns = int(expiry_date.timestamp() * 1_000_000_000)

        future = FuturesContract(
            instrument_id=future_id,
            raw_symbol=Symbol("ESH4"),
            asset_class=AssetClass.INDEX,
            exchange="XCME",
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.25"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying="ESH4",
            activation_ns=0,
            expiration_ns=expiry_ns,
            ts_event=0,
            ts_init=0,
        )
        call_option = OptionContract(
            instrument_id=call_id,
            raw_symbol=Symbol("ESH4C150"),
            asset_class=AssetClass.INDEX,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying="ESH4",
            option_kind=OptionKind.CALL,
            activation_ns=0,
            expiration_ns=expiry_ns,
            strike_price=Price.from_str("150.00"),
            ts_event=0,
            ts_init=0,
        )
        put_option = OptionContract(
            instrument_id=put_id,
            raw_symbol=Symbol("ESH4P150"),
            asset_class=AssetClass.INDEX,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying="ESH4",
            option_kind=OptionKind.PUT,
            activation_ns=0,
            expiration_ns=expiry_ns,
            strike_price=Price.from_str("150.00"),
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_instrument(future)
        self.cache.add_instrument(call_option)
        self.cache.add_instrument(put_option)

        call_price = Price.from_str("8.50")
        put_price = Price.from_str("3.33")
        self.cache.add_quote_tick(
            QuoteTick(
                instrument_id=call_id,
                bid_price=call_price,
                ask_price=call_price,
                bid_size=Quantity.from_int(100),
                ask_size=Quantity.from_int(100),
                ts_event=self.clock.timestamp_ns(),
                ts_init=self.clock.timestamp_ns(),
            ),
        )
        self.cache.add_quote_tick(
            QuoteTick(
                instrument_id=put_id,
                bid_price=put_price,
                ask_price=put_price,
                bid_size=Quantity.from_int(100),
                ask_size=Quantity.from_int(100),
                ts_event=self.clock.timestamp_ns(),
                ts_init=self.clock.timestamp_ns(),
            ),
        )

        greeks = self.greeks_calculator.instrument_greeks(
            instrument_id=call_id,
            flat_interest_rate=0.0425,
            cache_greeks=False,
            ts_event=self.clock.timestamp_ns(),
        )

        assert greeks is None

    def test_cache_futures_spread_returns_spread_to_reference_future(self):
        future_id = InstrumentId(Symbol("ESH4"), Venue("GLBX"))
        reference_future_id = InstrumentId(Symbol("ESM4"), Venue("GLBX"))
        call_id = InstrumentId(Symbol("ESH4C150"), Venue("GLBX"))
        put_id = InstrumentId(Symbol("ESH4P150"), Venue("GLBX"))
        expiry_date = datetime(2024, 3, 15, 16, 0, 0, tzinfo=UTC)
        expiry_ns = int(expiry_date.timestamp() * 1_000_000_000)

        future = FuturesContract(
            instrument_id=future_id,
            raw_symbol=Symbol("ESH4"),
            asset_class=AssetClass.INDEX,
            exchange="XCME",
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.25"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying="ESH4",
            activation_ns=0,
            expiration_ns=expiry_ns,
            ts_event=0,
            ts_init=0,
        )
        reference_future = FuturesContract(
            instrument_id=reference_future_id,
            raw_symbol=Symbol("ESM4"),
            asset_class=AssetClass.INDEX,
            exchange="XCME",
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.25"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying="ESM4",
            activation_ns=0,
            expiration_ns=expiry_ns,
            ts_event=0,
            ts_init=0,
        )
        call_option = OptionContract(
            instrument_id=call_id,
            raw_symbol=Symbol("ESH4C150"),
            asset_class=AssetClass.INDEX,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying="ESH4",
            option_kind=OptionKind.CALL,
            activation_ns=0,
            expiration_ns=expiry_ns,
            strike_price=Price.from_str("150.00"),
            ts_event=0,
            ts_init=0,
        )
        put_option = OptionContract(
            instrument_id=put_id,
            raw_symbol=Symbol("ESH4P150"),
            asset_class=AssetClass.INDEX,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying="ESH4",
            option_kind=OptionKind.PUT,
            activation_ns=0,
            expiration_ns=expiry_ns,
            strike_price=Price.from_str("150.00"),
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_instrument(future)
        self.cache.add_instrument(reference_future)
        self.cache.add_instrument(call_option)
        self.cache.add_instrument(put_option)
        self.cache.add_quote_tick(
            QuoteTick(
                instrument_id=reference_future_id,
                bid_price=Price.from_str("155.00"),
                ask_price=Price.from_str("155.00"),
                bid_size=Quantity.from_int(100),
                ask_size=Quantity.from_int(100),
                ts_event=self.clock.timestamp_ns(),
                ts_init=self.clock.timestamp_ns(),
            ),
        )
        self.cache.add_quote_tick(
            QuoteTick(
                instrument_id=call_id,
                bid_price=Price.from_str("8.50"),
                ask_price=Price.from_str("8.50"),
                bid_size=Quantity.from_int(100),
                ask_size=Quantity.from_int(100),
                ts_event=self.clock.timestamp_ns(),
                ts_init=self.clock.timestamp_ns(),
            ),
        )
        self.cache.add_quote_tick(
            QuoteTick(
                instrument_id=put_id,
                bid_price=Price.from_str("3.33"),
                ask_price=Price.from_str("3.33"),
                bid_size=Quantity.from_int(100),
                ask_size=Quantity.from_int(100),
                ts_event=self.clock.timestamp_ns(),
                ts_init=self.clock.timestamp_ns(),
            ),
        )

        cached_future_price = self.greeks_calculator.cache_futures_spread(
            call_instrument_id=call_id,
            put_instrument_id=put_id,
            futures_instrument_id=reference_future_id,
        )

        expected_underlying = 150.0 + exp(0.0425 * (30 / 365.25)) * (8.50 - 3.33)
        expected_cached_underlying = float(reference_future.make_price(expected_underlying))
        cached_underlying_price = self.greeks_calculator.get_cached_futures_spread_price(future_id)
        assert isinstance(cached_future_price, Price)
        assert round(float(cached_future_price), 12) == round(expected_cached_underlying, 12)
        assert isinstance(cached_underlying_price, Price)
        assert round(float(cached_underlying_price), 12) == round(expected_cached_underlying, 12)

    def test_instrument_greeks_uses_cached_futures_spread_when_underlying_price_missing(self):
        future_id = InstrumentId(Symbol("ESH4"), Venue("GLBX"))
        reference_future_id = InstrumentId(Symbol("ESM4"), Venue("GLBX"))
        call_id = InstrumentId(Symbol("ESH4C150"), Venue("GLBX"))
        put_id = InstrumentId(Symbol("ESH4P150"), Venue("GLBX"))
        target_call_id = InstrumentId(Symbol("ESH4C152"), Venue("GLBX"))
        expiry_date = datetime(2024, 3, 15, 16, 0, 0, tzinfo=UTC)
        expiry_ns = int(expiry_date.timestamp() * 1_000_000_000)

        future = FuturesContract(
            instrument_id=future_id,
            raw_symbol=Symbol("ESH4"),
            asset_class=AssetClass.INDEX,
            exchange="XCME",
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.25"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying="ESH4",
            activation_ns=0,
            expiration_ns=expiry_ns,
            ts_event=0,
            ts_init=0,
        )
        reference_future = FuturesContract(
            instrument_id=reference_future_id,
            raw_symbol=Symbol("ESM4"),
            asset_class=AssetClass.INDEX,
            exchange="XCME",
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.25"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying="ESM4",
            activation_ns=0,
            expiration_ns=expiry_ns,
            ts_event=0,
            ts_init=0,
        )
        call_option = OptionContract(
            instrument_id=call_id,
            raw_symbol=Symbol("ESH4C150"),
            asset_class=AssetClass.INDEX,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying="ESH4",
            option_kind=OptionKind.CALL,
            activation_ns=0,
            expiration_ns=expiry_ns,
            strike_price=Price.from_str("150.00"),
            ts_event=0,
            ts_init=0,
        )
        put_option = OptionContract(
            instrument_id=put_id,
            raw_symbol=Symbol("ESH4P150"),
            asset_class=AssetClass.INDEX,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying="ESH4",
            option_kind=OptionKind.PUT,
            activation_ns=0,
            expiration_ns=expiry_ns,
            strike_price=Price.from_str("150.00"),
            ts_event=0,
            ts_init=0,
        )
        target_call_option = OptionContract(
            instrument_id=target_call_id,
            raw_symbol=Symbol("ESH4C152"),
            asset_class=AssetClass.INDEX,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying="ESH4",
            option_kind=OptionKind.CALL,
            activation_ns=0,
            expiration_ns=expiry_ns,
            strike_price=Price.from_str("152.00"),
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_instrument(future)
        self.cache.add_instrument(reference_future)
        self.cache.add_instrument(call_option)
        self.cache.add_instrument(put_option)
        self.cache.add_instrument(target_call_option)
        self.cache.add_quote_tick(
            QuoteTick(
                instrument_id=reference_future_id,
                bid_price=Price.from_str("155.00"),
                ask_price=Price.from_str("155.00"),
                bid_size=Quantity.from_int(100),
                ask_size=Quantity.from_int(100),
                ts_event=self.clock.timestamp_ns(),
                ts_init=self.clock.timestamp_ns(),
            ),
        )
        self.cache.add_quote_tick(
            QuoteTick(
                instrument_id=call_id,
                bid_price=Price.from_str("8.50"),
                ask_price=Price.from_str("8.50"),
                bid_size=Quantity.from_int(100),
                ask_size=Quantity.from_int(100),
                ts_event=self.clock.timestamp_ns(),
                ts_init=self.clock.timestamp_ns(),
            ),
        )
        self.cache.add_quote_tick(
            QuoteTick(
                instrument_id=put_id,
                bid_price=Price.from_str("3.33"),
                ask_price=Price.from_str("3.33"),
                bid_size=Quantity.from_int(100),
                ask_size=Quantity.from_int(100),
                ts_event=self.clock.timestamp_ns(),
                ts_init=self.clock.timestamp_ns(),
            ),
        )
        self.cache.add_quote_tick(
            QuoteTick(
                instrument_id=target_call_id,
                bid_price=Price.from_str("6.75"),
                ask_price=Price.from_str("6.75"),
                bid_size=Quantity.from_int(100),
                ask_size=Quantity.from_int(100),
                ts_event=self.clock.timestamp_ns(),
                ts_init=self.clock.timestamp_ns(),
            ),
        )

        self.greeks_calculator.cache_futures_spread(
            call_instrument_id=call_id,
            put_instrument_id=put_id,
            futures_instrument_id=reference_future_id,
        )

        greeks = self.greeks_calculator.instrument_greeks(
            instrument_id=target_call_id,
            cache_greeks=False,
            ts_event=self.clock.timestamp_ns(),
        )

        expected_underlying = 150.0 + exp(0.0425 * (30 / 365.25)) * (8.50 - 3.33)
        assert greeks is not None
        assert round(greeks.underlying_price, 12) == round(
            float(reference_future.make_price(expected_underlying)),
            12,
        )

    def test_instrument_greeks_uses_index_price_for_index_underlying(self):
        future_id = InstrumentId(Symbol("ESH4"), Venue("GLBX"))
        call_id = InstrumentId(Symbol("ESH4C150"), Venue("GLBX"))
        expiry_date = datetime(2024, 3, 15, 16, 0, 0, tzinfo=UTC)
        expiry_ns = int(expiry_date.timestamp() * 1_000_000_000)

        future = FuturesContract(
            instrument_id=future_id,
            raw_symbol=Symbol("ESH4"),
            asset_class=AssetClass.INDEX,
            exchange="XCME",
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.25"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying="ESH4",
            activation_ns=0,
            expiration_ns=expiry_ns,
            ts_event=0,
            ts_init=0,
        )
        call_option = OptionContract(
            instrument_id=call_id,
            raw_symbol=Symbol("ESH4C150"),
            asset_class=AssetClass.INDEX,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying="ESH4",
            option_kind=OptionKind.CALL,
            activation_ns=0,
            expiration_ns=expiry_ns,
            strike_price=Price.from_str("150.00"),
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_instrument(future)
        self.cache.add_instrument(call_option)
        self.cache.add_index_price(
            IndexPriceUpdate(
                instrument_id=future_id,
                value=Price.from_str("157.25"),
                ts_event=self.clock.timestamp_ns(),
                ts_init=self.clock.timestamp_ns(),
            ),
        )
        self.cache.add_quote_tick(
            QuoteTick(
                instrument_id=call_id,
                bid_price=Price.from_str("8.50"),
                ask_price=Price.from_str("8.50"),
                bid_size=Quantity.from_int(100),
                ask_size=Quantity.from_int(100),
                ts_event=self.clock.timestamp_ns(),
                ts_init=self.clock.timestamp_ns(),
            ),
        )

        greeks = self.greeks_calculator.instrument_greeks(
            instrument_id=call_id,
            flat_interest_rate=0.0425,
            cache_greeks=False,
            ts_event=self.clock.timestamp_ns(),
        )

        assert greeks is not None
        assert greeks.underlying_price == 157.25

    def test_instrument_greeks_prefers_quote_over_index_price_for_index_future(self):
        future_id = InstrumentId(Symbol("ESH4"), Venue("GLBX"))
        call_id = InstrumentId(Symbol("ESH4C150"), Venue("GLBX"))
        expiry_date = datetime(2024, 3, 15, 16, 0, 0, tzinfo=UTC)
        expiry_ns = int(expiry_date.timestamp() * 1_000_000_000)

        future = FuturesContract(
            instrument_id=future_id,
            raw_symbol=Symbol("ESH4"),
            asset_class=AssetClass.INDEX,
            exchange="XCME",
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.25"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying="ESH4",
            activation_ns=0,
            expiration_ns=expiry_ns,
            ts_event=0,
            ts_init=0,
        )
        call_option = OptionContract(
            instrument_id=call_id,
            raw_symbol=Symbol("ESH4C150"),
            asset_class=AssetClass.INDEX,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying="ESH4",
            option_kind=OptionKind.CALL,
            activation_ns=0,
            expiration_ns=expiry_ns,
            strike_price=Price.from_str("150.00"),
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_instrument(future)
        self.cache.add_instrument(call_option)
        # Both a quote and an index price for the underlying future
        self.cache.add_quote_tick(
            QuoteTick(
                instrument_id=future_id,
                bid_price=Price.from_str("158.50"),
                ask_price=Price.from_str("159.50"),
                bid_size=Quantity.from_int(100),
                ask_size=Quantity.from_int(100),
                ts_event=self.clock.timestamp_ns(),
                ts_init=self.clock.timestamp_ns(),
            ),
        )
        self.cache.add_index_price(
            IndexPriceUpdate(
                instrument_id=future_id,
                value=Price.from_str("157.25"),
                ts_event=self.clock.timestamp_ns(),
                ts_init=self.clock.timestamp_ns(),
            ),
        )
        self.cache.add_quote_tick(
            QuoteTick(
                instrument_id=call_id,
                bid_price=Price.from_str("8.50"),
                ask_price=Price.from_str("8.50"),
                bid_size=Quantity.from_int(100),
                ask_size=Quantity.from_int(100),
                ts_event=self.clock.timestamp_ns(),
                ts_init=self.clock.timestamp_ns(),
            ),
        )

        greeks = self.greeks_calculator.instrument_greeks(
            instrument_id=call_id,
            flat_interest_rate=0.0425,
            cache_greeks=False,
            ts_event=self.clock.timestamp_ns(),
        )

        assert greeks is not None
        # Should use the MID quote (159.00), not the index price (157.25)
        assert greeks.underlying_price == 159.0

    def test_instrument_greeks_returns_none_when_opposite_option_price_missing_for_parity(self):
        future_id = InstrumentId(Symbol("ESH4"), Venue("GLBX"))
        call_id = InstrumentId(Symbol("ESH4C150"), Venue("GLBX"))
        expiry_date = datetime(2024, 3, 15, 16, 0, 0, tzinfo=UTC)
        expiry_ns = int(expiry_date.timestamp() * 1_000_000_000)

        future = FuturesContract(
            instrument_id=future_id,
            raw_symbol=Symbol("ESH4"),
            asset_class=AssetClass.INDEX,
            exchange="XCME",
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.25"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying="ESH4",
            activation_ns=0,
            expiration_ns=expiry_ns,
            ts_event=0,
            ts_init=0,
        )
        call_option = OptionContract(
            instrument_id=call_id,
            raw_symbol=Symbol("ESH4C150"),
            asset_class=AssetClass.INDEX,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying="ESH4",
            option_kind=OptionKind.CALL,
            activation_ns=0,
            expiration_ns=expiry_ns,
            strike_price=Price.from_str("150.00"),
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_instrument(future)
        self.cache.add_instrument(call_option)
        self.cache.add_quote_tick(
            QuoteTick(
                instrument_id=call_id,
                bid_price=Price.from_str("8.50"),
                ask_price=Price.from_str("8.50"),
                bid_size=Quantity.from_int(100),
                ask_size=Quantity.from_int(100),
                ts_event=self.clock.timestamp_ns(),
                ts_init=self.clock.timestamp_ns(),
            ),
        )

        greeks = self.greeks_calculator.instrument_greeks(
            instrument_id=call_id,
            flat_interest_rate=0.0425,
            cache_greeks=False,
            ts_event=self.clock.timestamp_ns(),
        )

        assert greeks is None

    def test_instrument_greeks_returns_none_when_parity_symbol_cannot_be_derived(self):
        future_id = InstrumentId(Symbol("ESH4"), Venue("GLBX"))
        broken_option_id = InstrumentId(Symbol("ESH4150"), Venue("GLBX"))
        expiry_date = datetime(2024, 3, 15, 16, 0, 0, tzinfo=UTC)
        expiry_ns = int(expiry_date.timestamp() * 1_000_000_000)

        future = FuturesContract(
            instrument_id=future_id,
            raw_symbol=Symbol("ESH4"),
            asset_class=AssetClass.INDEX,
            exchange="XCME",
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.25"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying="ESH4",
            activation_ns=0,
            expiration_ns=expiry_ns,
            ts_event=0,
            ts_init=0,
        )
        broken_option = OptionContract(
            instrument_id=broken_option_id,
            raw_symbol=Symbol("ESH4150"),
            asset_class=AssetClass.INDEX,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying="ESH4",
            option_kind=OptionKind.CALL,
            activation_ns=0,
            expiration_ns=expiry_ns,
            strike_price=Price.from_str("150.00"),
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_instrument(future)
        self.cache.add_instrument(broken_option)
        self.cache.add_quote_tick(
            QuoteTick(
                instrument_id=broken_option_id,
                bid_price=Price.from_str("8.50"),
                ask_price=Price.from_str("8.50"),
                bid_size=Quantity.from_int(100),
                ask_size=Quantity.from_int(100),
                ts_event=self.clock.timestamp_ns(),
                ts_init=self.clock.timestamp_ns(),
            ),
        )

        greeks = self.greeks_calculator.instrument_greeks(
            instrument_id=broken_option_id,
            flat_interest_rate=0.0425,
            cache_greeks=False,
            ts_event=self.clock.timestamp_ns(),
        )

        assert greeks is None

    def test_instrument_greeks_update_vol(self):
        # Test update_vol functionality - uses cached vol as initial guess for refinement
        underlying_price = Price.from_str("155.00")
        initial_option_price = Price.from_str("8.50")

        quote_underlying = QuoteTick(
            instrument_id=self.underlying_id,
            bid_price=underlying_price,
            ask_price=underlying_price,
            bid_size=Quantity.from_int(100),
            ask_size=Quantity.from_int(100),
            ts_event=self.clock.timestamp_ns(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.cache.add_quote_tick(quote_underlying)

        quote_option_initial = QuoteTick(
            instrument_id=self.option_id,
            bid_price=initial_option_price,
            ask_price=initial_option_price,
            bid_size=Quantity.from_int(100),
            ask_size=Quantity.from_int(100),
            ts_event=self.clock.timestamp_ns(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.cache.add_quote_tick(quote_option_initial)

        # Calculate and cache initial greeks
        initial_greeks = self.greeks_calculator.instrument_greeks(
            instrument_id=self.option_id,
            cache_greeks=True,
            ts_event=self.clock.timestamp_ns(),
        )

        initial_vol = initial_greeks.vol

        # Update option price (simulating market move)
        new_option_price = Price.from_str("9.00")
        quote_option_new = QuoteTick(
            instrument_id=self.option_id,
            bid_price=new_option_price,
            ask_price=new_option_price,
            bid_size=Quantity.from_int(100),
            ask_size=Quantity.from_int(100),
            ts_event=self.clock.timestamp_ns(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.cache.add_quote_tick(quote_option_new)

        # Use update_vol to refine volatility from cached value
        updated_greeks = self.greeks_calculator.instrument_greeks(
            instrument_id=self.option_id,
            update_vol=True,
            cache_greeks=False,
            ts_event=self.clock.timestamp_ns(),
        )

        # Expected updated price from computation
        assert abs(updated_greeks.price - 8.999732971191406) < 1e-3, (
            f"Updated price mismatch: {updated_greeks.price} vs 8.999732971191406"
        )
        # Vol should be updated based on new price (should be different from initial)
        assert abs(updated_greeks.vol - initial_vol) > 1e-3, (
            f"Vol should change when price changes: {updated_greeks.vol} vs {initial_vol}"
        )
        # Expected updated vol (~35.7%)
        assert abs(updated_greeks.vol - 0.3565638065) < 1e-3, (
            f"Updated vol mismatch: {updated_greeks.vol} vs 0.3565638065"
        )

    def test_instrument_greeks_update_vol_without_cache(self):
        # Test that update_vol falls back to standard calculation when no cache exists
        underlying_price = Price.from_str("155.00")
        option_price = Price.from_str("8.50")

        quote_underlying = QuoteTick(
            instrument_id=self.underlying_id,
            bid_price=underlying_price,
            ask_price=underlying_price,
            bid_size=Quantity.from_int(100),
            ask_size=Quantity.from_int(100),
            ts_event=self.clock.timestamp_ns(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.cache.add_quote_tick(quote_underlying)

        quote_option = QuoteTick(
            instrument_id=self.option_id,
            bid_price=option_price,
            ask_price=option_price,
            bid_size=Quantity.from_int(100),
            ask_size=Quantity.from_int(100),
            ts_event=self.clock.timestamp_ns(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.cache.add_quote_tick(quote_option)

        # Try update_vol without cached greeks - should fall back to standard calculation
        greeks = self.greeks_calculator.instrument_greeks(
            instrument_id=self.option_id,
            update_vol=True,  # No cached greeks available
            cache_greeks=False,
            ts_event=self.clock.timestamp_ns(),
        )

        # Should compute greeks using standard implied vol (same as without update_vol)
        # Expected values for 30 DTE, IV ~32.6%
        assert abs(greeks.price - 8.500007629394531) < 1e-3, (
            f"Price should match expected: {greeks.price} vs 8.500007629394531"
        )
        assert abs(greeks.vol - 0.3260962941) < 1e-5, (
            f"Vol should match expected: {greeks.vol} vs 0.3260962941"
        )

        # Compare with standard calculation (should be same when no cache exists)
        standard_greeks = self.greeks_calculator.instrument_greeks(
            instrument_id=self.option_id,
            update_vol=False,
            cache_greeks=False,
            ts_event=self.clock.timestamp_ns(),
        )
        assert abs(greeks.price - standard_greeks.price) < 1e-10, (
            f"Should match standard calculation: {greeks.price} vs {standard_greeks.price}"
        )
        assert abs(greeks.vol - standard_greeks.vol) < 1e-10, (
            f"Should match standard calculation: {greeks.vol} vs {standard_greeks.vol}"
        )

    def test_portfolio_greeks_update_vol(self):
        # Test portfolio_greeks with update_vol and non-zero position
        underlying_price = Price.from_str("155.00")
        initial_option_price = Price.from_str("8.50")

        quote_underlying = QuoteTick(
            instrument_id=self.underlying_id,
            bid_price=underlying_price,
            ask_price=underlying_price,
            bid_size=Quantity.from_int(100),
            ask_size=Quantity.from_int(100),
            ts_event=self.clock.timestamp_ns(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.cache.add_quote_tick(quote_underlying)

        quote_option_initial = QuoteTick(
            instrument_id=self.option_id,
            bid_price=initial_option_price,
            ask_price=initial_option_price,
            bid_size=Quantity.from_int(100),
            ask_size=Quantity.from_int(100),
            ts_event=self.clock.timestamp_ns(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.cache.add_quote_tick(quote_option_initial)

        # Create a position for the option
        order = self.order_factory.market(
            self.option_id,
            OrderSide.BUY,
            Quantity.from_int(1),
        )
        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)

        fill = TestEventStubs.order_filled(
            order,
            instrument=self.option,
            position_id=position_id,
            last_px=initial_option_price,
        )
        position = Position(instrument=self.option, fill=fill)
        self.cache.add_position(position, OmsType.HEDGING)

        # Calculate and cache initial greeks
        self.greeks_calculator.instrument_greeks(
            instrument_id=self.option_id,
            cache_greeks=True,
            ts_event=self.clock.timestamp_ns(),
        )

        # Update option price
        new_option_price = Price.from_str("9.00")
        quote_option_new = QuoteTick(
            instrument_id=self.option_id,
            bid_price=new_option_price,
            ask_price=new_option_price,
            bid_size=Quantity.from_int(100),
            ask_size=Quantity.from_int(100),
            ts_event=self.clock.timestamp_ns(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.cache.add_quote_tick(quote_option_new)

        # Calculate portfolio greeks with update_vol
        # portfolio_greeks() uses self._clock.timestamp_ns() internally for ts_event
        portfolio_greeks = self.greeks_calculator.portfolio_greeks(
            update_vol=True,
            cache_greeks=False,
        )

        # Portfolio greeks should have non-zero values from the position
        # Expected values: portfolio greeks = position quantity (1) * instrument greeks
        # Position is long 1 contract, so signed_qty = 1.0
        # Portfolio price = 1.0 * instrument_price (already includes multiplier)
        tol = 1e-2
        assert abs(portfolio_greeks.price - 899.9732971191406) < tol, (
            f"Portfolio price should match updated option price: {portfolio_greeks.price} vs 899.9732971191406"
        )
        # Portfolio delta should be non-zero (1.0 * instrument_delta from the position)
        assert abs(portfolio_greeks.delta) > 1e-3, (
            f"Portfolio delta should be non-zero: {portfolio_greeks.delta}"
        )
        # Portfolio gamma should be non-zero
        assert abs(portfolio_greeks.gamma) > 1e-5, (
            f"Portfolio gamma should be non-zero: {portfolio_greeks.gamma}"
        )
        # Portfolio vega should be non-zero
        assert abs(portfolio_greeks.vega) > 1e-3, (
            f"Portfolio vega should be non-zero: {portfolio_greeks.vega}"
        )
        # Portfolio theta should be non-zero (negative for long call)
        assert portfolio_greeks.theta < 0.0, (
            f"Portfolio theta should be negative for long call: {portfolio_greeks.theta}"
        )

    def test_instrument_greeks_option_pnl_uses_unscaled_open_price(self):
        underlying_price = Price.from_str("155.00")
        option_price = Price.from_str("9.00")

        self.cache.add_quote_tick(
            QuoteTick(
                instrument_id=self.underlying_id,
                bid_price=underlying_price,
                ask_price=underlying_price,
                bid_size=Quantity.from_int(100),
                ask_size=Quantity.from_int(100),
                ts_event=self.clock.timestamp_ns(),
                ts_init=self.clock.timestamp_ns(),
            ),
        )
        self.cache.add_quote_tick(
            QuoteTick(
                instrument_id=self.option_id,
                bid_price=option_price,
                ask_price=option_price,
                bid_size=Quantity.from_int(100),
                ask_size=Quantity.from_int(100),
                ts_event=self.clock.timestamp_ns(),
                ts_init=self.clock.timestamp_ns(),
            ),
        )

        order = self.order_factory.market(
            self.option_id,
            OrderSide.BUY,
            Quantity.from_int(1),
        )
        fill = TestEventStubs.order_filled(
            order,
            instrument=self.option,
            position_id=PositionId("P-2"),
            last_px=Price.from_str("8.50"),
        )
        position = Position(instrument=self.option, fill=fill)

        greeks = self.greeks_calculator.instrument_greeks(
            instrument_id=self.option_id,
            cache_greeks=False,
            ts_event=self.clock.timestamp_ns(),
            position=position,
        )

        assert greeks is not None
        assert greeks.pnl == greeks.price - position.avg_px_open
