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
        double prob_slippage = 0.0,
        random_seed: int | None = None,
        config = None,
    ) -> None:
        if config is not None:
            # Initialize from config
            prob_fill_on_limit = config.prob_fill_on_limit
            prob_slippage = config.prob_slippage
            random_seed = config.random_seed

        Condition.in_range(prob_fill_on_limit, 0.0, 1.0, "prob_fill_on_limit")
        Condition.in_range(prob_slippage, 0.0, 1.0, "prob_slippage")
        if random_seed is not None:
            Condition.type(random_seed, int, "random_seed")
            random.seed(random_seed)
        else:
            random.seed()

        self.prob_fill_on_limit = prob_fill_on_limit
        self.prob_slippage = prob_slippage

    cpdef bint is_limit_filled(self):
        """
        Return a value indicating whether a ``LIMIT`` order filled.

        Returns
        -------
        bool

        """
        return self._event_success(self.prob_fill_on_limit)

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


cdef class BestPriceFillModel(FillModel):
    """
    Fill model that executes all orders at the best available price.

    This model simulates optimistic market conditions where every order gets filled
    immediately at the best available price. Ideal for testing basic strategy logic.

    """

    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    ):
        """
        Return OrderBook with unlimited liquidity at best prices.
        """
        cdef:
            uint64_t UNLIMITED = 1_000_000  # Large enough to fill any order
            OrderBook book
            BookOrder bid_order
            BookOrder ask_order

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


cdef class OneTickSlippageFillModel(FillModel):
    """
    Fill model that forces exactly one tick of slippage for all orders.

    This model demonstrates how to create deterministic slippage by setting zero volume
    at best prices and unlimited volume one tick away.

    """

    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    ):
        """
        Return OrderBook with no volume at best prices, unlimited volume one tick away.
        """
        cdef:
            Price tick = instrument.price_increment
            uint64_t UNLIMITED = 1_000_000
            OrderBook book
            BookOrder bid_order
            BookOrder ask_order

        book = OrderBook(
            instrument_id=instrument.id,
            book_type=BookType.L2_MBP,
        )

        # Only add liquidity one tick away from best price (simulates no liquidity at best)
        # By not adding any orders at best_bid/best_ask, we guarantee slippage
        bid_order = BookOrder(
            side=OrderSide.BUY,
            price=best_bid.sub(tick),
            size=Quantity(UNLIMITED, instrument.size_precision),
            order_id=1,
        )
        ask_order = BookOrder(
            side=OrderSide.SELL,
            price=best_ask.add(tick),
            size=Quantity(UNLIMITED, instrument.size_precision),
            order_id=2,
        )

        book.add(bid_order, 0, 0)
        book.add(ask_order, 0, 0)

        return book


cdef class TwoTierFillModel(FillModel):
    """
    Fill model with two-tier pricing: first 10 contracts at best price, remainder one tick worse.

    This model simulates basic market depth behavior and provides realistic simulation
    of basic market impact for small to medium orders.
    """

    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    ):
        """
        Return OrderBook with two-tier liquidity structure.
        """
        cdef:
            Price tick = instrument.price_increment
            uint64_t UNLIMITED = 1_000_000
            OrderBook book
            BookOrder bid_order_tier1
            BookOrder ask_order_tier1
            BookOrder bid_order_tier2
            BookOrder ask_order_tier2

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
            price=best_bid.sub(tick),
            size=Quantity(UNLIMITED, instrument.size_precision),
            order_id=3,
        )
        ask_order_tier2 = BookOrder(
            side=OrderSide.SELL,
            price=best_ask.add(tick),
            size=Quantity(UNLIMITED, instrument.size_precision),
            order_id=4,
        )

        book.add(bid_order_tier1, 0, 0)
        book.add(ask_order_tier1, 0, 0)
        book.add(bid_order_tier2, 0, 0)
        book.add(ask_order_tier2, 0, 0)

        return book


cdef class ProbabilisticFillModel(FillModel):
    """
    Fill model that replicates the current probabilistic behavior.

    This model demonstrates how to implement the existing FillModel's probabilistic
    behavior using the new simulation approach: 50% chance of best price fill,
    50% chance of one tick slippage.

    """

    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    ):
        """
        Return OrderBook based on probabilistic logic.
        """
        cdef:
            Price tick = instrument.price_increment
            uint64_t UNLIMITED = 1_000_000
            OrderBook book
            BookOrder bid_order
            BookOrder ask_order

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
                price=best_bid.sub(tick),
                size=Quantity(UNLIMITED, instrument.size_precision),
                order_id=1,
            )
            ask_order = BookOrder(
                side=OrderSide.SELL,
                price=best_ask.add(tick),
                size=Quantity(UNLIMITED, instrument.size_precision),
                order_id=2,
            )

        book.add(bid_order, 0, 0)
        book.add(ask_order, 0, 0)

        return book


cdef class SizeAwareFillModel(FillModel):
    """
    Fill model that applies different execution models based on order size.

    Small orders (<=10) get good liquidity at best prices. Large orders experience price
    impact with partial fills at worse prices.

    """

    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    ):
        """
        Return OrderBook with size-dependent liquidity.
        """
        cdef:
            Price tick = instrument.price_increment
            OrderBook book
            Quantity threshold_qty
            Quantity remaining_qty
            BookOrder bid_order
            BookOrder ask_order
            BookOrder bid_order_tier1
            BookOrder ask_order_tier1
            BookOrder bid_order_tier2
            BookOrder ask_order_tier2

        book = OrderBook(
            instrument_id=instrument.id,
            book_type=BookType.L2_MBP,
        )

        threshold_qty = Quantity(10, instrument.size_precision)
        if order.quantity._mem.raw <= threshold_qty._mem.raw:
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
            remaining_qty = order.quantity.sub(threshold_qty)

            # First level: 10 contracts at best price
            bid_order_tier1 = BookOrder(
                side=OrderSide.BUY,
                price=best_bid,
                size=threshold_qty,
                order_id=1,
            )
            ask_order_tier1 = BookOrder(
                side=OrderSide.SELL,
                price=best_ask,
                size=threshold_qty,
                order_id=2,
            )

            # Second level: remainder at worse price
            bid_order_tier2 = BookOrder(
                side=OrderSide.BUY,
                price=best_bid.sub(tick),
                size=remaining_qty,
                order_id=3,
            )
            ask_order_tier2 = BookOrder(
                side=OrderSide.SELL,
                price=best_ask.add(tick),
                size=remaining_qty,
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


cdef class LimitOrderPartialFillModel(FillModel):
    """
    Fill model that simulates partial fills for limit orders.

    When price touches the limit level, only fills maximum 5 contracts of the order
    quantity, modeling typical limit order queue behavior.

    """

    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    ):
        """
        Return OrderBook with limited fills at limit prices.
        """
        cdef:
            Price tick = instrument.price_increment
            uint64_t UNLIMITED = 1_000_000
            OrderBook book
            BookOrder bid_order_tier1
            BookOrder ask_order_tier1
            BookOrder bid_order_tier2
            BookOrder ask_order_tier2

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
            price=best_bid.sub(tick),
            size=Quantity(UNLIMITED, instrument.size_precision),
            order_id=3,
        )
        ask_order_tier2 = BookOrder(
            side=OrderSide.SELL,
            price=best_ask.add(tick),
            size=Quantity(UNLIMITED, instrument.size_precision),
            order_id=4,
        )

        book.add(bid_order_tier1, 0, 0)
        book.add(ask_order_tier1, 0, 0)
        book.add(bid_order_tier2, 0, 0)
        book.add(ask_order_tier2, 0, 0)

        return book


cdef class ThreeTierFillModel(FillModel):
    """
    Fill model with three-tier pricing for realistic market depth simulation.

    Distributes 100-contract order fills across three price levels:
    - 50 contracts at best price
    - 30 contracts 1 tick worse
    - 20 contracts 2 ticks worse

    """

    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    ):
        """
        Return OrderBook with three-tier liquidity structure.
        """
        cdef:
            Price tick = instrument.price_increment
            Price two_ticks
            OrderBook book
            BookOrder bid_order_tier1
            BookOrder ask_order_tier1
            BookOrder bid_order_tier2
            BookOrder ask_order_tier2
            BookOrder bid_order_tier3
            BookOrder ask_order_tier3

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
            price=best_bid.sub(tick),
            size=Quantity(30, instrument.size_precision),
            order_id=3,
        )
        ask_order_tier2 = BookOrder(
            side=OrderSide.SELL,
            price=best_ask.add(tick),
            size=Quantity(30, instrument.size_precision),
            order_id=4,
        )

        # Level 3: 20 contracts 2 ticks worse
        two_ticks = tick.add(tick)
        bid_order_tier3 = BookOrder(
            side=OrderSide.BUY,
            price=best_bid.sub(two_ticks),
            size=Quantity(20, instrument.size_precision),
            order_id=5,
        )
        ask_order_tier3 = BookOrder(
            side=OrderSide.SELL,
            price=best_ask.add(two_ticks),
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


cdef class MarketHoursFillModel(FillModel):
    """
    Fill model that simulates varying market conditions based on time.

    Implements wider spreads during low liquidity periods (e.g., outside market hours).
    Essential for strategies that trade across different market sessions.

    """

    def __init__(
        self,
        double prob_fill_on_limit = 1.0,
        double prob_slippage = 0.0,
        random_seed = None,
    ):
        super().__init__(prob_fill_on_limit, prob_slippage, random_seed)
        # In a real implementation, you would track market hours
        self._is_low_liquidity = False  # Simplified for example

    cpdef bint is_low_liquidity_period(self):
        """
        Check if current time is during low liquidity period.
        """
        # In a real implementation, this would check actual market hours
        # For demo purposes, we'll use a simple flag
        return self._is_low_liquidity

    cpdef void set_low_liquidity_period(self, bint is_low_liquidity):
        """
        Set the liquidity period for testing purposes.
        """
        self._is_low_liquidity = is_low_liquidity

    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    ):
        """
        Return OrderBook with time-dependent liquidity.
        """
        cdef:
            Price tick = instrument.price_increment
            uint64_t NORMAL_VOLUME = 500
            OrderBook book
            BookOrder bid_order
            BookOrder ask_order

        book = OrderBook(
            instrument_id=instrument.id,
            book_type=BookType.L2_MBP,
        )

        if self._is_low_liquidity:
            # During low liquidity: wider spreads (1 tick worse)
            bid_order = BookOrder(
                side=OrderSide.BUY,
                price=best_bid.sub(tick),
                size=Quantity(NORMAL_VOLUME, instrument.size_precision),
                order_id=1,
            )
            ask_order = BookOrder(
                side=OrderSide.SELL,
                price=best_ask.add(tick),
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


cdef class VolumeSensitiveFillModel(FillModel):
    """
    Fill model that adjusts liquidity based on recent trading volume.

    Creates realistic market depth based on actual market activity by using recent bar
    volume data to determine available liquidity.

    """

    def __init__(
        self,
        double prob_fill_on_limit = 1.0,
        double prob_slippage = 0.0,
        random_seed = None,
    ):
        super().__init__(prob_fill_on_limit, prob_slippage, random_seed)
        self._recent_volume = 1000.0  # Default volume for demo

    cpdef void set_recent_volume(self, double volume):
        """
        Set recent volume for testing purposes.
        """
        self._recent_volume = volume

    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    ):
        """
        Return OrderBook with volume-based liquidity.
        """
        cdef:
            Price tick = instrument.price_increment
            uint64_t UNLIMITED = 1_000_000
            uint64_t available_volume
            OrderBook book
            BookOrder bid_order_tier1
            BookOrder ask_order_tier1
            BookOrder bid_order_tier2
            BookOrder ask_order_tier2

        book = OrderBook(
            instrument_id=instrument.id,
            book_type=BookType.L2_MBP,
        )

        # Minimum 1 to avoid zero-size orders being silently dropped by OrderBook
        available_volume = max(<uint64_t>1, <uint64_t>(self._recent_volume * 0.25))

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

        bid_order_tier2 = BookOrder(
            side=OrderSide.BUY,
            price=best_bid.sub(tick),
            size=Quantity(UNLIMITED, instrument.size_precision),
            order_id=3,
        )
        ask_order_tier2 = BookOrder(
            side=OrderSide.SELL,
            price=best_ask.add(tick),
            size=Quantity(UNLIMITED, instrument.size_precision),
            order_id=4,
        )

        book.add(bid_order_tier1, 0, 0)
        book.add(ask_order_tier1, 0, 0)
        book.add(bid_order_tier2, 0, 0)
        book.add(ask_order_tier2, 0, 0)

        return book


cdef class CompetitionAwareFillModel(FillModel):
    """
    Fill model that simulates market competition effects.

    Makes only a percentage of visible liquidity actually available, reflecting
    realistic conditions where multiple traders compete for the same liquidity.

    """

    def __init__(
        self,
        double prob_fill_on_limit = 1.0,
        double prob_slippage = 0.0,
        random_seed = None,
        double liquidity_factor = 0.3,
    ):
        super().__init__(prob_fill_on_limit, prob_slippage, random_seed)
        self.liquidity_factor = liquidity_factor

    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    ):
        """
        Return OrderBook with competition-adjusted liquidity.
        """
        cdef:
            double typical_bid_volume = 1000.0
            double typical_ask_volume = 1000.0
            uint64_t available_bid
            uint64_t available_ask
            OrderBook book
            BookOrder bid_order
            BookOrder ask_order

        # Minimum 1 to avoid zero-size orders being silently dropped by OrderBook
        available_bid = max(<uint64_t>1, <uint64_t>(typical_bid_volume * self.liquidity_factor))
        available_ask = max(<uint64_t>1, <uint64_t>(typical_ask_volume * self.liquidity_factor))

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
