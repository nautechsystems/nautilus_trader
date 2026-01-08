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

from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
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
        self.msgbus = MessageBus(
            trader_id=TestIdStubs.trader_id(),
            clock=self.clock,
        )
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
            msgbus=self.msgbus,
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
        )

        # Exact expected values from computation (tolerance 1e-5 for f64 precision)
        # Expected values computed from market price of 8.50 with underlying at 155.00
        assert abs(greeks.price - 850.0015258789) < 1e-5, (
            f"Price mismatch: {greeks.price} vs 850.0015258789"
        )
        assert abs(greeks.vol - 1.7764886189) < 1e-5, f"Vol mismatch: {greeks.vol} vs 1.7764886189"
        assert abs(greeks.delta - 65.5138254166) < 1e-5, (
            f"Delta mismatch: {greeks.delta} vs 65.5138254166"
        )
        assert abs(greeks.gamma - 2.5565279648) < 1e-5, (
            f"Gamma mismatch: {greeks.gamma} vs 2.5565279648"
        )
        assert abs(greeks.vega - 2.9873502254) < 1e-5, (
            f"Vega mismatch: {greeks.vega} vs 2.9873502254"
        )
        assert abs(greeks.theta - (-265.2507847258)) < 1e-4, (
            f"Theta mismatch: {greeks.theta} vs -265.2507847258"
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
        )

        cached_vol = greeks1.vol

        # Use cached greeks
        greeks2 = self.greeks_calculator.instrument_greeks(
            instrument_id=self.option_id,
            use_cached_greeks=True,
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
        )

        # Exact expected updated price from computation
        assert abs(updated_greeks.price - 899.9732971191) < 1e-5, (
            f"Updated price mismatch: {updated_greeks.price} vs 899.9732971191"
        )
        # Vol should be updated based on new price (should be different from initial)
        assert abs(updated_greeks.vol - initial_vol) > 1e-3, (
            f"Vol should change when price changes: {updated_greeks.vol} vs {initial_vol}"
        )
        # Exact expected updated vol from computation (tolerance 1e-4 for refinement precision)
        assert abs(updated_greeks.vol - 1.9428999424) < 1e-4, (
            f"Updated vol mismatch: {updated_greeks.vol} vs 1.9428999424"
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
        )

        # Should compute greeks using standard implied vol (same as without update_vol)
        # Exact expected values from computation
        assert abs(greeks.price - 850.0015258789) < 1e-5, (
            f"Price should match expected: {greeks.price} vs 850.0015258789"
        )
        assert abs(greeks.vol - 1.7764886189) < 1e-5, (
            f"Vol should match expected: {greeks.vol} vs 1.7764886189"
        )

        # Compare with standard calculation (should be same when no cache exists)
        standard_greeks = self.greeks_calculator.instrument_greeks(
            instrument_id=self.option_id,
            update_vol=False,
            cache_greeks=False,
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
        portfolio_greeks = self.greeks_calculator.portfolio_greeks(
            update_vol=True,
            cache_greeks=False,
        )

        # Portfolio greeks should have non-zero values from the position
        # Expected values: portfolio greeks = position quantity (1) * instrument greeks
        # Position is long 1 contract, so signed_qty = 1.0
        # Portfolio price = 1.0 * instrument_price (already includes multiplier)
        tol = 1e-2
        assert abs(portfolio_greeks.price - 899.9732971191) < tol, (
            f"Portfolio price should match updated option price: {portfolio_greeks.price} vs 899.9732971191"
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
