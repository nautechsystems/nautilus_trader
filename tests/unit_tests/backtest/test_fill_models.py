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
"""
Tests for the enhanced FillModel functionality with order book simulation.
"""

from nautilus_trader.backtest.models import BestPriceFillModel
from nautilus_trader.backtest.models import CompetitionAwareFillModel
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import LimitOrderPartialFillModel
from nautilus_trader.backtest.models import MarketHoursFillModel
from nautilus_trader.backtest.models import OneTickSlippageFillModel
from nautilus_trader.backtest.models import ProbabilisticFillModel
from nautilus_trader.backtest.models import SizeAwareFillModel
from nautilus_trader.backtest.models import ThreeTierFillModel
from nautilus_trader.backtest.models import TwoTierFillModel
from nautilus_trader.backtest.models import VolumeSensitiveFillModel
from nautilus_trader.common.component import TestClock
from nautilus_trader.core.rust.model import BookType
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.execution import TestExecStubs


class TestEnhancedFillModels:
    def setup_method(self):
        # Common test setup
        self.clock = TestClock()
        self.instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        self.venue = Venue("SIM")

    def test_default_fill_model_returns_none(self):
        """
        Test that default FillModel returns None for simulation.
        """
        # Arrange
        fill_model = FillModel()
        best_bid = Price.from_str("1.00000")
        best_ask = Price.from_str("1.00010")
        order = TestExecStubs.market_order(instrument=self.instrument)

        # Act
        result = fill_model.get_orderbook_for_fill_simulation(
            self.instrument,
            order,
            best_bid,
            best_ask,
        )

        # Assert
        assert result is None

    def test_best_price_fill_model_creates_unlimited_liquidity(self):
        """
        Test BestPriceFillModel creates OrderBook with unlimited liquidity at best
        prices.
        """
        # Arrange
        fill_model = BestPriceFillModel()
        best_bid = Price.from_str("1.00000")
        best_ask = Price.from_str("1.00010")
        order = TestExecStubs.market_order(instrument=self.instrument)

        # Act
        result = fill_model.get_orderbook_for_fill_simulation(
            self.instrument,
            order,
            best_bid,
            best_ask,
        )

        # Assert
        assert result is not None
        assert isinstance(result, OrderBook)
        assert result.instrument_id == self.instrument.id
        assert result.book_type == BookType.L2_MBP

        # Check that there's liquidity at best prices
        bids = list(result.bids())
        asks = list(result.asks())

        assert len(bids) >= 1
        assert len(asks) >= 1
        assert bids[0].price == best_bid
        assert asks[0].price == best_ask
        assert bids[0].size() == 1_000_000  # UNLIMITED
        assert asks[0].size() == 1_000_000  # UNLIMITED

    def test_one_tick_slippage_model_creates_slippage(self):
        """
        Test OneTickSlippageFillModel creates OrderBook with guaranteed slippage.
        """
        # Arrange
        fill_model = OneTickSlippageFillModel()
        best_bid = Price.from_str("1.00000")
        best_ask = Price.from_str("1.00010")
        order = TestExecStubs.market_order(instrument=self.instrument)

        # Act
        result = fill_model.get_orderbook_for_fill_simulation(
            self.instrument,
            order,
            best_bid,
            best_ask,
        )

        # Assert
        assert result is not None
        assert isinstance(result, OrderBook)

        bids = list(result.bids())
        asks = list(result.asks())

        # Should have exactly 1 level per side (one tick away from best)
        # No liquidity at best price guarantees slippage
        assert len(bids) == 1
        assert len(asks) == 1

        # Only level should be one tick away from best price with unlimited volume
        tick = self.instrument.price_increment
        assert bids[0].price == best_bid - tick
        assert asks[0].price == best_ask + tick
        assert bids[0].size() == 1_000_000
        assert asks[0].size() == 1_000_000

    def test_two_tier_fill_model_creates_tiered_liquidity(self):
        """
        Test TwoTierFillModel creates OrderBook with two-tier liquidity.
        """
        # Arrange
        fill_model = TwoTierFillModel()
        best_bid = Price.from_str("1.00000")
        best_ask = Price.from_str("1.00010")
        order = TestExecStubs.market_order(instrument=self.instrument)

        # Act
        result = fill_model.get_orderbook_for_fill_simulation(
            self.instrument,
            order,
            best_bid,
            best_ask,
        )

        # Assert
        assert result is not None
        bids = list(result.bids())
        asks = list(result.asks())

        assert len(bids) >= 2
        assert len(asks) >= 2

        # First tier: 10 contracts at best price
        assert bids[0].price == best_bid
        assert asks[0].price == best_ask
        assert bids[0].size() == 10
        assert asks[0].size() == 10

        # Second tier: unlimited contracts one tick worse
        tick = self.instrument.price_increment
        assert bids[1].price == best_bid - tick
        assert asks[1].price == best_ask + tick
        assert bids[1].size() == 1_000_000
        assert asks[1].size() == 1_000_000

    def test_size_aware_fill_model_small_order(self):
        """
        Test SizeAwareFillModel handles small orders differently.
        """
        # Arrange
        fill_model = SizeAwareFillModel()
        best_bid = Price.from_str("1.00000")
        best_ask = Price.from_str("1.00010")
        small_order = TestExecStubs.market_order(
            instrument=self.instrument,
            quantity=Quantity.from_int(5),
        )  # Small order

        # Act
        result = fill_model.get_orderbook_for_fill_simulation(
            self.instrument,
            small_order,
            best_bid,
            best_ask,
        )

        # Assert
        assert result is not None
        bids = list(result.bids())
        asks = list(result.asks())

        # Small orders should get good liquidity at best prices
        assert len(bids) == 1
        assert len(asks) == 1
        assert bids[0].price == best_bid
        assert asks[0].price == best_ask
        assert bids[0].size() == 50
        assert asks[0].size() == 50

    def test_size_aware_fill_model_large_order(self):
        """
        Test SizeAwareFillModel handles large orders with price impact.
        """
        # Arrange
        fill_model = SizeAwareFillModel()
        best_bid = Price.from_str("1.00000")
        best_ask = Price.from_str("1.00010")
        large_order = TestExecStubs.market_order(
            instrument=self.instrument,
            quantity=Quantity.from_int(50),
        )  # Large order

        # Act
        result = fill_model.get_orderbook_for_fill_simulation(
            self.instrument,
            large_order,
            best_bid,
            best_ask,
        )

        # Assert
        assert result is not None
        bids = list(result.bids())
        asks = list(result.asks())

        # Large orders should experience price impact
        assert len(bids) >= 2
        assert len(asks) >= 2

        # First level: 10 contracts at best price
        assert bids[0].price == best_bid
        assert asks[0].price == best_ask
        assert bids[0].size() == 10
        assert asks[0].size() == 10

        # Second level: remainder at worse price
        tick = self.instrument.price_increment
        remaining_qty = large_order.quantity.as_double() - 10
        assert bids[1].price == best_bid - tick
        assert asks[1].price == best_ask + tick
        assert bids[1].size() == remaining_qty
        assert asks[1].size() == remaining_qty

    def test_backward_compatibility_with_existing_fill_model(self):
        """
        Test that existing FillModel behavior is preserved when simulation returns None.
        """
        # This test would require integration with the matching engine
        # For now, we just verify the method exists and returns None by default
        fill_model = FillModel()
        best_bid = Price.from_str("1.00000")
        best_ask = Price.from_str("1.00010")
        order = TestExecStubs.market_order(instrument=self.instrument)

        result = fill_model.get_orderbook_for_fill_simulation(
            self.instrument,
            order,
            best_bid,
            best_ask,
        )

        assert result is None  # Default behavior should return None

    def test_fill_model_with_different_instruments(self):
        """
        Test that fill models work with different instrument types.
        """
        # Arrange
        crypto_instrument = TestInstrumentProvider.btcusdt_binance()
        fill_model = BestPriceFillModel()
        best_bid = Price.from_str("50000.00")
        best_ask = Price.from_str("50001.00")
        order = TestExecStubs.market_order(instrument=crypto_instrument)

        # Act
        result = fill_model.get_orderbook_for_fill_simulation(
            crypto_instrument,
            order,
            best_bid,
            best_ask,
        )

        # Assert
        assert result is not None
        assert result.instrument_id == crypto_instrument.id

        bids = list(result.bids())
        asks = list(result.asks())
        assert bids[0].price == best_bid
        assert asks[0].price == best_ask

    def test_probabilistic_fill_model_creates_order_book(self):
        """
        Test ProbabilisticFillModel creates OrderBook (either at best or one tick away).
        """
        # Arrange
        fill_model = ProbabilisticFillModel()
        best_bid = Price.from_str("1.00000")
        best_ask = Price.from_str("1.00010")
        order = TestExecStubs.market_order(instrument=self.instrument)
        tick = self.instrument.price_increment

        # Act
        result = fill_model.get_orderbook_for_fill_simulation(
            self.instrument,
            order,
            best_bid,
            best_ask,
        )

        # Assert
        assert result is not None
        assert isinstance(result, OrderBook)

        bids = list(result.bids())
        asks = list(result.asks())

        assert len(bids) == 1
        assert len(asks) == 1

        # Should be either at best price or one tick away (50/50 probability)
        assert bids[0].price in (best_bid, best_bid - tick)
        assert asks[0].price in (best_ask, best_ask + tick)
        assert bids[0].size() == 1_000_000
        assert asks[0].size() == 1_000_000

    def test_limit_order_partial_fill_model_creates_tiered_liquidity(self):
        """
        Test LimitOrderPartialFillModel creates OrderBook with max 5 contracts at best.
        """
        # Arrange
        fill_model = LimitOrderPartialFillModel()
        best_bid = Price.from_str("1.00000")
        best_ask = Price.from_str("1.00010")
        order = TestExecStubs.market_order(instrument=self.instrument)

        # Act
        result = fill_model.get_orderbook_for_fill_simulation(
            self.instrument,
            order,
            best_bid,
            best_ask,
        )

        # Assert
        assert result is not None
        bids = list(result.bids())
        asks = list(result.asks())

        assert len(bids) == 2
        assert len(asks) == 2

        # First tier: max 5 contracts at best price
        assert bids[0].price == best_bid
        assert asks[0].price == best_ask
        assert bids[0].size() == 5
        assert asks[0].size() == 5

        # Second tier: unlimited one tick worse
        tick = self.instrument.price_increment
        assert bids[1].price == best_bid - tick
        assert asks[1].price == best_ask + tick
        assert bids[1].size() == 1_000_000
        assert asks[1].size() == 1_000_000

    def test_three_tier_fill_model_creates_three_level_liquidity(self):
        """
        Test ThreeTierFillModel creates OrderBook with 50/30/20 distribution.
        """
        # Arrange
        fill_model = ThreeTierFillModel()
        best_bid = Price.from_str("1.00000")
        best_ask = Price.from_str("1.00010")
        order = TestExecStubs.market_order(instrument=self.instrument)

        # Act
        result = fill_model.get_orderbook_for_fill_simulation(
            self.instrument,
            order,
            best_bid,
            best_ask,
        )

        # Assert
        assert result is not None
        bids = list(result.bids())
        asks = list(result.asks())

        assert len(bids) == 3
        assert len(asks) == 3

        tick = self.instrument.price_increment

        # Level 1: 50 contracts at best price
        assert bids[0].price == best_bid
        assert asks[0].price == best_ask
        assert bids[0].size() == 50
        assert asks[0].size() == 50

        # Level 2: 30 contracts 1 tick worse
        assert bids[1].price == best_bid - tick
        assert asks[1].price == best_ask + tick
        assert bids[1].size() == 30
        assert asks[1].size() == 30

        # Level 3: 20 contracts 2 ticks worse
        two_ticks = tick + tick
        assert bids[2].price == best_bid - two_ticks
        assert asks[2].price == best_ask + two_ticks
        assert bids[2].size() == 20
        assert asks[2].size() == 20

    def test_market_hours_fill_model_normal_hours(self):
        """
        Test MarketHoursFillModel creates normal liquidity during normal hours.
        """
        # Arrange
        fill_model = MarketHoursFillModel()
        fill_model.set_low_liquidity_period(False)
        best_bid = Price.from_str("1.00000")
        best_ask = Price.from_str("1.00010")
        order = TestExecStubs.market_order(instrument=self.instrument)

        # Act
        result = fill_model.get_orderbook_for_fill_simulation(
            self.instrument,
            order,
            best_bid,
            best_ask,
        )

        # Assert
        assert result is not None
        bids = list(result.bids())
        asks = list(result.asks())

        assert len(bids) == 1
        assert len(asks) == 1

        # Normal hours: liquidity at best prices
        assert bids[0].price == best_bid
        assert asks[0].price == best_ask
        assert bids[0].size() == 500
        assert asks[0].size() == 500

    def test_market_hours_fill_model_low_liquidity_period(self):
        """
        Test MarketHoursFillModel creates wider spreads during low liquidity.
        """
        # Arrange
        fill_model = MarketHoursFillModel()
        fill_model.set_low_liquidity_period(True)
        best_bid = Price.from_str("1.00000")
        best_ask = Price.from_str("1.00010")
        order = TestExecStubs.market_order(instrument=self.instrument)

        # Act
        result = fill_model.get_orderbook_for_fill_simulation(
            self.instrument,
            order,
            best_bid,
            best_ask,
        )

        # Assert
        assert result is not None
        bids = list(result.bids())
        asks = list(result.asks())

        assert len(bids) == 1
        assert len(asks) == 1

        # Low liquidity: wider spreads (1 tick worse)
        tick = self.instrument.price_increment
        assert bids[0].price == best_bid - tick
        assert asks[0].price == best_ask + tick
        assert bids[0].size() == 500
        assert asks[0].size() == 500

    def test_volume_sensitive_fill_model_default_volume(self):
        """
        Test VolumeSensitiveFillModel creates liquidity based on default volume.
        """
        # Arrange
        fill_model = VolumeSensitiveFillModel()  # Default volume is 1000
        best_bid = Price.from_str("1.00000")
        best_ask = Price.from_str("1.00010")
        order = TestExecStubs.market_order(instrument=self.instrument)

        # Act
        result = fill_model.get_orderbook_for_fill_simulation(
            self.instrument,
            order,
            best_bid,
            best_ask,
        )

        # Assert
        assert result is not None
        bids = list(result.bids())
        asks = list(result.asks())

        assert len(bids) == 2
        assert len(asks) == 2

        # First tier: 25% of 1000 = 250 at best price
        assert bids[0].price == best_bid
        assert asks[0].price == best_ask
        assert bids[0].size() == 250
        assert asks[0].size() == 250

        # Second tier: unlimited one tick worse
        tick = self.instrument.price_increment
        assert bids[1].price == best_bid - tick
        assert asks[1].price == best_ask + tick

    def test_volume_sensitive_fill_model_custom_volume(self):
        """
        Test VolumeSensitiveFillModel creates liquidity based on custom volume.
        """
        # Arrange
        fill_model = VolumeSensitiveFillModel()
        fill_model.set_recent_volume(400.0)  # 25% = 100
        best_bid = Price.from_str("1.00000")
        best_ask = Price.from_str("1.00010")
        order = TestExecStubs.market_order(instrument=self.instrument)

        # Act
        result = fill_model.get_orderbook_for_fill_simulation(
            self.instrument,
            order,
            best_bid,
            best_ask,
        )

        # Assert
        assert result is not None
        bids = list(result.bids())
        asks = list(result.asks())

        # First tier: 25% of 400 = 100 at best price
        assert bids[0].price == best_bid
        assert asks[0].price == best_ask
        assert bids[0].size() == 100
        assert asks[0].size() == 100

    def test_volume_sensitive_fill_model_minimum_volume(self):
        """
        Test VolumeSensitiveFillModel enforces minimum volume of 1.
        """
        # Arrange
        fill_model = VolumeSensitiveFillModel()
        fill_model.set_recent_volume(1.0)  # 25% = 0.25, should clamp to 1
        best_bid = Price.from_str("1.00000")
        best_ask = Price.from_str("1.00010")
        order = TestExecStubs.market_order(instrument=self.instrument)

        # Act
        result = fill_model.get_orderbook_for_fill_simulation(
            self.instrument,
            order,
            best_bid,
            best_ask,
        )

        # Assert
        assert result is not None
        bids = list(result.bids())
        asks = list(result.asks())

        # Should have minimum 1 at best price (not 0)
        assert bids[0].price == best_bid
        assert asks[0].price == best_ask
        assert bids[0].size() >= 1
        assert asks[0].size() >= 1

    def test_competition_aware_fill_model_default_factor(self):
        """
        Test CompetitionAwareFillModel creates liquidity with default 30% factor.
        """
        # Arrange
        fill_model = CompetitionAwareFillModel()  # Default factor is 0.3
        best_bid = Price.from_str("1.00000")
        best_ask = Price.from_str("1.00010")
        order = TestExecStubs.market_order(instrument=self.instrument)

        # Act
        result = fill_model.get_orderbook_for_fill_simulation(
            self.instrument,
            order,
            best_bid,
            best_ask,
        )

        # Assert
        assert result is not None
        bids = list(result.bids())
        asks = list(result.asks())

        assert len(bids) == 1
        assert len(asks) == 1

        # 30% of typical 1000 = 300 available
        assert bids[0].price == best_bid
        assert asks[0].price == best_ask
        assert bids[0].size() == 300
        assert asks[0].size() == 300

    def test_competition_aware_fill_model_custom_factor(self):
        """
        Test CompetitionAwareFillModel creates liquidity with custom factor.
        """
        # Arrange
        fill_model = CompetitionAwareFillModel(liquidity_factor=0.5)
        best_bid = Price.from_str("1.00000")
        best_ask = Price.from_str("1.00010")
        order = TestExecStubs.market_order(instrument=self.instrument)

        # Act
        result = fill_model.get_orderbook_for_fill_simulation(
            self.instrument,
            order,
            best_bid,
            best_ask,
        )

        # Assert
        assert result is not None
        bids = list(result.bids())
        asks = list(result.asks())

        # 50% of typical 1000 = 500 available
        assert bids[0].price == best_bid
        assert asks[0].price == best_ask
        assert bids[0].size() == 500
        assert asks[0].size() == 500

    def test_competition_aware_fill_model_minimum_liquidity(self):
        """
        Test CompetitionAwareFillModel enforces minimum liquidity of 1.
        """
        # Arrange
        fill_model = CompetitionAwareFillModel(liquidity_factor=0.0001)  # Very small
        best_bid = Price.from_str("1.00000")
        best_ask = Price.from_str("1.00010")
        order = TestExecStubs.market_order(instrument=self.instrument)

        # Act
        result = fill_model.get_orderbook_for_fill_simulation(
            self.instrument,
            order,
            best_bid,
            best_ask,
        )

        # Assert
        assert result is not None
        bids = list(result.bids())
        asks = list(result.asks())

        # Should have minimum 1 at best price (not 0)
        assert bids[0].price == best_bid
        assert asks[0].price == best_ask
        assert bids[0].size() >= 1
        assert asks[0].size() >= 1
