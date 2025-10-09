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

import random

from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport BookType
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.model.book cimport BookOrder
from nautilus_trader.model.book cimport OrderBook
from nautilus_trader.model.functions cimport liquidity_side_to_str
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef class FillModel:
    """
    Provides probabilistic modeling for order fill dynamics including probability
    of fills and slippage by order type.

    Parameters
    ----------
    prob_fill_on_limit : double
        The probability of limit order filling if the market rests on its price.
    prob_fill_on_stop : double
        The probability of stop orders filling if the market rests on its price.
    prob_slippage : double
        The probability of order fill prices slipping by one tick.
    random_seed : int, optional
        The random seed (if None then no random seed).
    config : FillModelConfig, optional
        The configuration for the model.

    Raises
    ------
    ValueError
        If any probability argument is not within range [0, 1].
    TypeError
        If `random_seed` is not None and not of type `int`.
    """

    def __init__(
        self,
        double prob_fill_on_limit = 1.0,
        double prob_fill_on_stop = 1.0,
        double prob_slippage = 0.0,
        random_seed: int | None = None,
        config = None,
    ) -> None:
        if config is not None:
            # Initialize from config
            prob_fill_on_limit = config.prob_fill_on_limit
            prob_fill_on_stop = config.prob_fill_on_stop
            prob_slippage = config.prob_slippage
            random_seed = config.random_seed

        Condition.in_range(prob_fill_on_limit, 0.0, 1.0, "prob_fill_on_limit")
        Condition.in_range(prob_fill_on_stop, 0.0, 1.0, "prob_fill_on_stop")
        Condition.in_range(prob_slippage, 0.0, 1.0, "prob_slippage")
        if random_seed is not None:
            Condition.type(random_seed, int, "random_seed")
            random.seed(random_seed)
        else:
            random.seed()

        self.prob_fill_on_limit = prob_fill_on_limit
        self.prob_fill_on_stop = prob_fill_on_stop
        self.prob_slippage = prob_slippage

    cpdef bint is_limit_filled(self):
        """
        Return a value indicating whether a ``LIMIT`` order filled.

        Returns
        -------
        bool

        """
        return self._event_success(self.prob_fill_on_limit)

    cpdef bint is_stop_filled(self):
        """
        Return a value indicating whether a ``STOP-MARKET`` order filled.

        Returns
        -------
        bool

        """
        return self._event_success(self.prob_fill_on_stop)

    cpdef bint is_slipped(self):
        """
        Return a value indicating whether an order fill slipped.

        Returns
        -------
        bool

        """
        return self._event_success(self.prob_slippage)

    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    ):
        """
        Return a simulated OrderBook for fill simulation.

        This method allows custom fill models to provide their own liquidity
        simulation by returning a custom OrderBook that represents the expected
        market liquidity. The matching engine will use this simulated OrderBook
        to determine fills.

        The default implementation returns None, which means the matching engine
        will use its standard fill logic (maintaining backward compatibility).

        Parameters
        ----------
        instrument : Instrument
            The instrument being traded.
        order : Order
            The order to simulate fills for.
        best_bid : Price
            The current best bid price.
        best_ask : Price
            The current best ask price.

        Returns
        -------
        OrderBook or None
            The simulated OrderBook for fill simulation, or None to use default logic.

        """
        return None  # Default implementation - use existing fill logic

    cdef bint _event_success(self, double probability):
        # Return a result indicating whether an event occurred based on the
        # given probability of the event occurring [0, 1].
        if probability == 0:
            return False
        elif probability == 1:
            return True
        else:
            return probability >= random.random()


class BestPriceFillModel(FillModel):
    """
    Fill model that executes all orders at the best available price.

    This model simulates optimistic market conditions where every order gets filled
    immediately at the best available price. Ideal for testing basic strategy logic.

    """

    def get_orderbook_for_fill_simulation(
        self,
        instrument: Instrument,
        order: Order,
        best_bid: Price,
        best_ask: Price,
    ) -> OrderBook | None:
        """
        Return OrderBook with unlimited liquidity at best prices.
        """
        UNLIMITED = 1_000_000  # Large enough to fill any order

        book = OrderBook(
            instrument_id=instrument.id,
            book_type=BookType.L2_MBP,
        )

        # Add unlimited volume at best prices
        bid_order = BookOrder(
            side=OrderSide.BUY,
            price=best_bid,
            size=Quantity(UNLIMITED, instrument.size_precision),
            order_id=1,
        )
        ask_order = BookOrder(
            side=OrderSide.SELL,
            price=best_ask,
            size=Quantity(UNLIMITED, instrument.size_precision),
            order_id=2,
        )

        book.add(bid_order, 0, 0)
        book.add(ask_order, 0, 0)

        return book


class OneTickSlippageFillModel(FillModel):
    """
    Fill model that forces exactly one tick of slippage for all orders.

    This model demonstrates how to create deterministic slippage by setting zero volume
    at best prices and unlimited volume one tick away.

    """

    def get_orderbook_for_fill_simulation(
        self,
        instrument: Instrument,
        order: Order,
        best_bid: Price,
        best_ask: Price,
    ) -> OrderBook | None:
        """
        Return OrderBook with no volume at best prices, unlimited volume one tick away.
        """
        tick = instrument.price_increment
        UNLIMITED = 1_000_000

        book = OrderBook(
            instrument_id=instrument.id,
            book_type=BookType.L2_MBP,
        )

        # Only add liquidity one tick away from best price (simulates no liquidity at best)
        # By not adding any orders at best_bid/best_ask, we guarantee slippage
        bid_order = BookOrder(
            side=OrderSide.BUY,
            price=Price(best_bid.as_double() - tick.as_double(), instrument.price_precision),
            size=Quantity(UNLIMITED, instrument.size_precision),
            order_id=1,
        )
        ask_order = BookOrder(
            side=OrderSide.SELL,
            price=Price(best_ask.as_double() + tick.as_double(), instrument.price_precision),
            size=Quantity(UNLIMITED, instrument.size_precision),
            order_id=2,
        )

        book.add(bid_order, 0, 0)
        book.add(ask_order, 0, 0)

        return book


class TwoTierFillModel(FillModel):
    """
    Fill model with two-tier pricing: first 10 contracts at best price, remainder one tick worse.

    This model simulates basic market depth behavior and provides realistic simulation
    of basic market impact for small to medium orders.
    """

    def get_orderbook_for_fill_simulation(
        self,
        instrument: Instrument,
        order: Order,
        best_bid: Price,
        best_ask: Price,
    ) -> OrderBook | None:
        """
        Return OrderBook with two-tier liquidity structure.
        """
        tick = instrument.price_increment
        UNLIMITED = 1_000_000

        book = OrderBook(
            instrument_id=instrument.id,
            book_type=BookType.L2_MBP,
        )

        # First tier: 10 contracts at best price
        bid_order_tier1 = BookOrder(
            side=OrderSide.BUY,
            price=best_bid,
            size=Quantity(10, instrument.size_precision),
            order_id=1,
        )
        ask_order_tier1 = BookOrder(
            side=OrderSide.SELL,
            price=best_ask,
            size=Quantity(10, instrument.size_precision),
            order_id=2,
        )

        # Second tier: unlimited contracts one tick worse
        bid_order_tier2 = BookOrder(
            side=OrderSide.BUY,
            price=Price(best_bid.as_double() - tick.as_double(), instrument.price_precision),
            size=Quantity(UNLIMITED, instrument.size_precision),
            order_id=3,
        )
        ask_order_tier2 = BookOrder(
            side=OrderSide.SELL,
            price=Price(best_ask.as_double() + tick.as_double(), instrument.price_precision),
            size=Quantity(UNLIMITED, instrument.size_precision),
            order_id=4,
        )

        book.add(bid_order_tier1, 0, 0)
        book.add(ask_order_tier1, 0, 0)
        book.add(bid_order_tier2, 0, 0)
        book.add(ask_order_tier2, 0, 0)

        return book


class ProbabilisticFillModel(FillModel):
    """
    Fill model that replicates the current probabilistic behavior.

    This model demonstrates how to implement the existing FillModel's probabilistic
    behavior using the new simulation approach: 50% chance of best price fill,
    50% chance of one tick slippage.

    """

    def get_orderbook_for_fill_simulation(
        self,
        instrument: Instrument,
        order: Order,
        best_bid: Price,
        best_ask: Price,
    ) -> OrderBook | None:
        """
        Return OrderBook based on probabilistic logic.
        """
        tick = instrument.price_increment
        UNLIMITED = 1_000_000

        book = OrderBook(
            instrument_id=instrument.id,
            book_type=BookType.L2_MBP,
        )

        if random.random() < 0.5:  # noqa: S311
            # 50% chance: fill at best price
            bid_order = BookOrder(
                side=OrderSide.BUY,
                price=best_bid,
                size=Quantity(UNLIMITED, instrument.size_precision),
                order_id=1,
            )
            ask_order = BookOrder(
                side=OrderSide.SELL,
                price=best_ask,
                size=Quantity(UNLIMITED, instrument.size_precision),
                order_id=2,
            )
        else:
            # 50% chance: one tick slippage
            bid_order = BookOrder(
                side=OrderSide.BUY,
                price=Price(best_bid.as_double() - tick.as_double(), instrument.price_precision),
                size=Quantity(UNLIMITED, instrument.size_precision),
                order_id=1,
            )
            ask_order = BookOrder(
                side=OrderSide.SELL,
                price=Price(best_ask.as_double() + tick.as_double(), instrument.price_precision),
                size=Quantity(UNLIMITED, instrument.size_precision),
                order_id=2,
            )

        book.add(bid_order, 0, 0)
        book.add(ask_order, 0, 0)

        return book


class SizeAwareFillModel(FillModel):
    """
    Fill model that applies different execution models based on order size.

    Small orders (<=10) get good liquidity at best prices. Large orders experience price
    impact with partial fills at worse prices.

    """

    def get_orderbook_for_fill_simulation(
        self,
        instrument: Instrument,
        order: Order,
        best_bid: Price,
        best_ask: Price,
    ) -> OrderBook | None:
        """
        Return OrderBook with size-dependent liquidity.
        """
        tick = instrument.price_increment

        book = OrderBook(
            instrument_id=instrument.id,
            book_type=BookType.L2_MBP,
        )

        if order.quantity.as_double() <= 10:
            # Small orders: good liquidity at best prices
            bid_order = BookOrder(
                side=OrderSide.BUY,
                price=best_bid,
                size=Quantity(50, instrument.size_precision),
                order_id=1,
            )
            ask_order = BookOrder(
                side=OrderSide.SELL,
                price=best_ask,
                size=Quantity(50, instrument.size_precision),
                order_id=2,
            )
        else:
            # Large orders: price impact
            remaining_qty = order.quantity.as_double() - 10

            # First level: 10 contracts at best price
            bid_order_tier1 = BookOrder(
                side=OrderSide.BUY,
                price=best_bid,
                size=Quantity(10, instrument.size_precision),
                order_id=1,
            )
            ask_order_tier1 = BookOrder(
                side=OrderSide.SELL,
                price=best_ask,
                size=Quantity(10, instrument.size_precision),
                order_id=2,
            )

            # Second level: remainder at worse price
            bid_order_tier2 = BookOrder(
                side=OrderSide.BUY,
                price=Price(best_bid.as_double() - tick.as_double(), instrument.price_precision),
                size=Quantity(remaining_qty, instrument.size_precision),
                order_id=3,
            )
            ask_order_tier2 = BookOrder(
                side=OrderSide.SELL,
                price=Price(best_ask.as_double() + tick.as_double(), instrument.price_precision),
                size=Quantity(remaining_qty, instrument.size_precision),
                order_id=4,
            )

            book.add(bid_order_tier1, 0, 0)
            book.add(ask_order_tier1, 0, 0)
            book.add(bid_order_tier2, 0, 0)
            book.add(ask_order_tier2, 0, 0)

            return book

        book.add(bid_order, 0, 0)
        book.add(ask_order, 0, 0)

        return book


class LimitOrderPartialFillModel(FillModel):
    """
    Fill model that simulates partial fills for limit orders.

    When price touches the limit level, only fills maximum 5 contracts of the order
    quantity, modeling typical limit order queue behavior.

    """

    def get_orderbook_for_fill_simulation(
        self,
        instrument: Instrument,
        order: Order,
        best_bid: Price,
        best_ask: Price,
    ) -> OrderBook | None:
        """
        Return OrderBook with limited fills at limit prices.
        """
        tick = instrument.price_increment
        UNLIMITED = 1_000_000

        book = OrderBook(
            instrument_id=instrument.id,
            book_type=BookType.L2_MBP,
        )

        # Max 5 contracts fill if market price touches limit price
        bid_order_tier1 = BookOrder(
            side=OrderSide.BUY,
            price=best_bid,
            size=Quantity(5, instrument.size_precision),
            order_id=1,
        )
        ask_order_tier1 = BookOrder(
            side=OrderSide.SELL,
            price=best_ask,
            size=Quantity(5, instrument.size_precision),
            order_id=2,
        )

        # Second level acts as price buffer
        bid_order_tier2 = BookOrder(
            side=OrderSide.BUY,
            price=Price(best_bid.as_double() - tick.as_double(), instrument.price_precision),
            size=Quantity(UNLIMITED, instrument.size_precision),
            order_id=3,
        )
        ask_order_tier2 = BookOrder(
            side=OrderSide.SELL,
            price=Price(best_ask.as_double() + tick.as_double(), instrument.price_precision),
            size=Quantity(UNLIMITED, instrument.size_precision),
            order_id=4,
        )

        book.add(bid_order_tier1, 0, 0)
        book.add(ask_order_tier1, 0, 0)
        book.add(bid_order_tier2, 0, 0)
        book.add(ask_order_tier2, 0, 0)

        return book


class ThreeTierFillModel(FillModel):
    """
    Fill model with three-tier pricing for realistic market depth simulation.

    Distributes 100-contract order fills across three price levels:
    - 50 contracts at best price
    - 30 contracts 1 tick worse
    - 20 contracts 2 ticks worse

    """

    def get_orderbook_for_fill_simulation(
        self,
        instrument: Instrument,
        order: Order,
        best_bid: Price,
        best_ask: Price,
    ) -> OrderBook | None:
        """
        Return OrderBook with three-tier liquidity structure.
        """
        tick = instrument.price_increment

        book = OrderBook(
            instrument_id=instrument.id,
            book_type=BookType.L2_MBP,
        )

        # Level 1: 50 contracts at best price
        bid_order_tier1 = BookOrder(
            side=OrderSide.BUY,
            price=best_bid,
            size=Quantity(50, instrument.size_precision),
            order_id=1,
        )
        ask_order_tier1 = BookOrder(
            side=OrderSide.SELL,
            price=best_ask,
            size=Quantity(50, instrument.size_precision),
            order_id=2,
        )

        # Level 2: 30 contracts 1 tick worse
        bid_order_tier2 = BookOrder(
            side=OrderSide.BUY,
            price=Price(best_bid.as_double() - tick.as_double(), instrument.price_precision),
            size=Quantity(30, instrument.size_precision),
            order_id=3,
        )
        ask_order_tier2 = BookOrder(
            side=OrderSide.SELL,
            price=Price(best_ask.as_double() + tick.as_double(), instrument.price_precision),
            size=Quantity(30, instrument.size_precision),
            order_id=4,
        )

        # Level 3: 20 contracts 2 ticks worse
        bid_order_tier3 = BookOrder(
            side=OrderSide.BUY,
            price=Price(best_bid.as_double() - (tick.as_double() * 2), instrument.price_precision),
            size=Quantity(20, instrument.size_precision),
            order_id=5,
        )
        ask_order_tier3 = BookOrder(
            side=OrderSide.SELL,
            price=Price(best_ask.as_double() + (tick.as_double() * 2), instrument.price_precision),
            size=Quantity(20, instrument.size_precision),
            order_id=6,
        )

        book.add(bid_order_tier1, 0, 0)
        book.add(ask_order_tier1, 0, 0)
        book.add(bid_order_tier2, 0, 0)
        book.add(ask_order_tier2, 0, 0)
        book.add(bid_order_tier3, 0, 0)
        book.add(ask_order_tier3, 0, 0)

        return book


class MarketHoursFillModel(FillModel):
    """
    Fill model that simulates varying market conditions based on time.

    Implements wider spreads during low liquidity periods (e.g., outside market hours).
    Essential for strategies that trade across different market sessions.

    """

    def __init__(
        self,
        prob_fill_on_limit=1.0,
        prob_fill_on_stop=1.0,
        prob_slippage=0.0,
        random_seed=None,
    ):
        super().__init__(prob_fill_on_limit, prob_fill_on_stop, prob_slippage, random_seed)
        # In a real implementation, you would track market hours
        self._is_low_liquidity = False  # Simplified for example

    def is_low_liquidity_period(self) -> bool:
        """
        Check if current time is during low liquidity period.
        """
        # In a real implementation, this would check actual market hours
        # For demo purposes, we'll use a simple flag
        return self._is_low_liquidity

    def set_low_liquidity_period(self, is_low_liquidity: bool):
        """
        Set the liquidity period for testing purposes.
        """
        self._is_low_liquidity = is_low_liquidity

    def get_orderbook_for_fill_simulation(
        self,
        instrument: Instrument,
        order: Order,
        best_bid: Price,
        best_ask: Price,
    ) -> OrderBook | None:
        """
        Return OrderBook with time-dependent liquidity.
        """
        tick = instrument.price_increment
        NORMAL_VOLUME = 500

        book = OrderBook(
            instrument_id=instrument.id,
            book_type=BookType.L2_MBP,
        )

        if self.is_low_liquidity_period():
            # During low liquidity: wider spreads (1 tick worse)
            bid_order = BookOrder(
                side=OrderSide.BUY,
                price=Price(best_bid.as_double() - tick.as_double(), instrument.price_precision),
                size=Quantity(NORMAL_VOLUME, instrument.size_precision),
                order_id=1,
            )
            ask_order = BookOrder(
                side=OrderSide.SELL,
                price=Price(best_ask.as_double() + tick.as_double(), instrument.price_precision),
                size=Quantity(NORMAL_VOLUME, instrument.size_precision),
                order_id=2,
            )
        else:
            # Normal hours: standard liquidity
            bid_order = BookOrder(
                side=OrderSide.BUY,
                price=best_bid,
                size=Quantity(NORMAL_VOLUME, instrument.size_precision),
                order_id=1,
            )
            ask_order = BookOrder(
                side=OrderSide.SELL,
                price=best_ask,
                size=Quantity(NORMAL_VOLUME, instrument.size_precision),
                order_id=2,
            )

        book.add(bid_order, 0, 0)
        book.add(ask_order, 0, 0)

        return book


class VolumeSensitiveFillModel(FillModel):
    """
    Fill model that adjusts liquidity based on recent trading volume.

    Creates realistic market depth based on actual market activity by using recent bar
    volume data to determine available liquidity.

    """

    def __init__(
        self,
        prob_fill_on_limit=1.0,
        prob_fill_on_stop=1.0,
        prob_slippage=0.0,
        random_seed=None,
    ):
        super().__init__(prob_fill_on_limit, prob_fill_on_stop, prob_slippage, random_seed)
        self._recent_volume = 1000.0  # Default volume for demo

    def set_recent_volume(self, volume: float):
        """
        Set recent volume for testing purposes.
        """
        self._recent_volume = volume

    def get_orderbook_for_fill_simulation(
        self,
        instrument: Instrument,
        order: Order,
        best_bid: Price,
        best_ask: Price,
    ) -> OrderBook | None:
        """
        Return OrderBook with volume-based liquidity.
        """
        tick = instrument.price_increment
        UNLIMITED = 1_000_000

        book = OrderBook(
            instrument_id=instrument.id,
            book_type=BookType.L2_MBP,
        )

        # Available liquidity is 25% of recent average volume
        available_volume = int(self._recent_volume * 0.25)

        bid_order_tier1 = BookOrder(
            side=OrderSide.BUY,
            price=best_bid,
            size=Quantity(available_volume, instrument.size_precision),
            order_id=1,
        )
        ask_order_tier1 = BookOrder(
            side=OrderSide.SELL,
            price=best_ask,
            size=Quantity(available_volume, instrument.size_precision),
            order_id=2,
        )

        # Unlimited volume one tick worse
        bid_order_tier2 = BookOrder(
            side=OrderSide.BUY,
            price=Price(best_bid.as_double() - tick.as_double(), instrument.price_precision),
            size=Quantity(UNLIMITED, instrument.size_precision),
            order_id=3,
        )
        ask_order_tier2 = BookOrder(
            side=OrderSide.SELL,
            price=Price(best_ask.as_double() + tick.as_double(), instrument.price_precision),
            size=Quantity(UNLIMITED, instrument.size_precision),
            order_id=4,
        )

        book.add(bid_order_tier1, 0, 0)
        book.add(ask_order_tier1, 0, 0)
        book.add(bid_order_tier2, 0, 0)
        book.add(ask_order_tier2, 0, 0)

        return book


class CompetitionAwareFillModel(FillModel):
    """
    Fill model that simulates market competition effects.

    Makes only a percentage of visible liquidity actually available, reflecting
    realistic conditions where multiple traders compete for the same liquidity.

    """

    def __init__(
        self,
        prob_fill_on_limit=1.0,
        prob_fill_on_stop=1.0,
        prob_slippage=0.0,
        random_seed=None,
        liquidity_factor=0.3,
    ):
        super().__init__(prob_fill_on_limit, prob_fill_on_stop, prob_slippage, random_seed)
        self.liquidity_factor = liquidity_factor  # Can access 30% of visible liquidity by default

    def get_orderbook_for_fill_simulation(
        self,
        instrument: Instrument,
        order: Order,
        best_bid: Price,
        best_ask: Price,
    ) -> OrderBook | None:
        """
        Return OrderBook with competition-adjusted liquidity.
        """
        # In a real implementation, you would get the actual current orderbook
        # For demo purposes, we'll simulate typical market depth
        typical_bid_volume = 1000.0
        typical_ask_volume = 1000.0

        available_bid = int(typical_bid_volume * self.liquidity_factor)
        available_ask = int(typical_ask_volume * self.liquidity_factor)

        book = OrderBook(
            instrument_id=instrument.id,
            book_type=BookType.L2_MBP,
        )

        bid_order = BookOrder(
            side=OrderSide.BUY,
            price=best_bid,
            size=Quantity(available_bid, instrument.size_precision),
            order_id=1,
        )
        ask_order = BookOrder(
            side=OrderSide.SELL,
            price=best_ask,
            size=Quantity(available_ask, instrument.size_precision),
            order_id=2,
        )

        book.add(bid_order, 0, 0)
        book.add(ask_order, 0, 0)

        return book
