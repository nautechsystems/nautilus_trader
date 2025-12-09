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

from typing import Callable

import numpy as np

cimport numpy as np
from cpython.datetime cimport timedelta
from libc.stdint cimport uint64_t

from datetime import timedelta

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.common.component cimport TimeEvent
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport InstrumentClass
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.greeks cimport GreeksCalculator
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport generic_spread_id_to_list
from nautilus_trader.model.identifiers cimport is_generic_spread_id


cdef class SpreadQuoteAggregator(Component):
    """
    Provides a spread quote generator for creating synthetic quotes from component instruments.

    The generator subscribes to quotes from component instruments of a spread and generates
    averaged quotes for the spread instrument.

    Parameters
    ----------
    spread_instrument_id : InstrumentId
        The spread instrument ID to generate quotes for.
    handler : Callable[[QuoteTick], None]
        The quote handler for the generator.
    cache : CacheFacade
        The cache facade for accessing market data.
    """

    def __init__(
        self,
        InstrumentId spread_instrument_id not None,
        handler not None: Callable[[QuoteTick], None],
        MessageBus msgbus not None,
        CacheFacade cache not None,
        Clock clock not None,
        int update_interval_seconds = 60,
        list spread_legs = None,
    ):
        Condition.is_true(is_generic_spread_id(spread_instrument_id), "instrument_id must be a spread")

        super().__init__(clock=clock, msgbus=msgbus)
        self._handler = handler
        self._cache = cache
        self._log = Logger(name=f"{type(self).__name__}")

        self._spread_instrument_id = spread_instrument_id

        # Use provided spread_legs if available, otherwise parse from instrument_id
        if spread_legs is not None:
            self._components = spread_legs
        else:
            self._components = generic_spread_id_to_list(spread_instrument_id)

        self._greeks_calculator = GreeksCalculator(msgbus, cache, clock)

        self._component_ids = [component[0] for component in self._components]
        self._ratios = np.array([component[1] for component in self._components])
        self._n_components = len(self._components)
        self._mid_prices = np.zeros(self._n_components)
        self._bid_prices = np.zeros(self._n_components)
        self._ask_prices = np.zeros(self._n_components)
        self._vegas = np.zeros(self._n_components)
        self._bid_ask_spreads = np.zeros(self._n_components)
        self._bid_sizes = np.zeros(self._n_components)
        self._ask_sizes = np.zeros(self._n_components)

        self._spread_instrument = self._cache.instrument(self._spread_instrument_id)
        if self._spread_instrument is not None:
            self._is_futures_spread = self._spread_instrument.instrument_class == InstrumentClass.FUTURES_SPREAD
        else:
            self._is_futures_spread = False

        self._update_interval_seconds = update_interval_seconds
        self._timer_name = f"spread_quote_timer_{self._spread_instrument_id}"
        self._set_build_timer()

    cdef void _set_build_timer(self):
        self._clock.set_timer(
            name=self._timer_name,
            interval=timedelta(seconds=self._update_interval_seconds),
            callback=self._build_quote,
            start_time=None,  # Start immediately
            stop_time=None,   # Run indefinitely
            allow_past=True,  # Allow past start times
            fire_immediately=True,  # Fire immediately when timer is set
        )

    cpdef void stop(self):
        self._clock.cancel_timer(self._timer_name)

    cdef void _build_quote(self, TimeEvent event):
        if self._spread_instrument is None:
            self._log.error(f"Cannot find spread instrument {self._spread_instrument_id}")
            return

        # Track missing components for better error reporting
        cdef list missing_quotes = []
        cdef list missing_greeks = []

        # Calculate component values
        for i, component_id in enumerate(self._component_ids):
            component_quote = self._cache.quote_tick(component_id)
            if component_quote is None:
                missing_quotes.append(str(component_id))
                continue

            ask_price = component_quote.ask_price.as_double()
            bid_price = component_quote.bid_price.as_double()

            self._bid_prices[i] = bid_price
            self._ask_prices[i] = ask_price
            self._bid_sizes[i] = component_quote.bid_size.as_double()
            self._ask_sizes[i] = component_quote.ask_size.as_double()

            if not self._is_futures_spread:
                self._mid_prices[i] = (ask_price + bid_price) * 0.5
                self._bid_ask_spreads[i] = ask_price - bid_price
                greeks_data = self._greeks_calculator.instrument_greeks(
                    component_id,
                    percent_greeks=True,
                    use_cached_greeks=True,  # Use cached greeks for testing
                    vega_time_weight_base=30, # 30-day time weight base
                )
                if greeks_data is None:
                    missing_greeks.append(str(component_id))
                    continue

                self._vegas[i] = greeks_data.vega

        # Check if we have all required data (use debug for timer-driven recurring conditions)
        if missing_quotes:
            self._log.error(
                f"Missing quotes for spread {self._spread_instrument_id} components: {', '.join(missing_quotes)}"
            )
            return

        cdef tuple price_result
        if self._is_futures_spread:
            price_result = self._create_futures_spread_prices()
        else:
            if missing_greeks:
                self._log.warning(
                    f"Missing greeks for spread {self._spread_instrument_id} components: {', '.join(missing_greeks)}"
                )
                return
            else:
                price_result = self._create_option_spread_prices()

        spread_quote = self._create_quote_tick_from_raw_prices(price_result[0], price_result[1], event.ts_event)

        # Send quote to the backtest engine so it reaches a venue and a matching engine
        self._msgbus.send(endpoint=f"SimulatedExchange.spread_quote.{self._spread_instrument_id.venue}", msg=spread_quote)

        # Send the spread quote to the data engine *after* it's possibly processed by a matching engine
        # like in the main backtesting loop in the backtest engine
        self._handler(spread_quote)

    cdef tuple _create_option_spread_prices(self):
        # Calculate bid ask spread of option spread
        # Use np.divide with where clause to handle zero vegas safely
        vega_multipliers = np.divide(
            self._bid_ask_spreads,
            self._vegas,
            out=np.zeros_like(self._vegas),
            where=self._vegas != 0
        )

        # Filter out zero multipliers before taking mean
        non_zero_multipliers = vega_multipliers[vega_multipliers != 0]
        if len(non_zero_multipliers) == 0:
            self._log.warning(
                f"All vegas are zero for spread {self._spread_instrument_id}, cannot generate spread quote"
            )
            return self._create_futures_spread_prices()

        vega_multiplier = np.abs(non_zero_multipliers).mean()
        spread_vega = abs(np.dot(self._vegas, self._ratios))

        bid_ask_spread = spread_vega * vega_multiplier
        self._log.debug(f"{self._bid_ask_spreads=}, {self._vegas=}, {vega_multipliers=}, "
                        f"{spread_vega=}, {vega_multiplier=}, {bid_ask_spread=}")

        # Calculate raw bid/ask prices
        spread_mid_price = (self._mid_prices * self._ratios).sum()
        raw_bid_price = spread_mid_price - bid_ask_spread * 0.5
        raw_ask_price = spread_mid_price + bid_ask_spread * 0.5

        return (raw_bid_price, raw_ask_price)

    cdef tuple _create_futures_spread_prices(self):
        # Calculate spread ask: for positive ratios use ask, for negative ratios use bid
        # Calculate spread bid: for positive ratios use bid, for negative ratios use ask

        cdef double raw_ask_price = 0.0
        cdef double raw_bid_price = 0.0

        cdef int i
        for i in range(self._n_components):
            if self._ratios[i] >= 0:
                raw_ask_price += self._ratios[i] * self._ask_prices[i]
                raw_bid_price += self._ratios[i] * self._bid_prices[i]
            else:
                raw_ask_price += self._ratios[i] * self._bid_prices[i]
                raw_bid_price += self._ratios[i] * self._ask_prices[i]

        return (raw_bid_price, raw_ask_price)

    cdef QuoteTick _create_quote_tick_from_raw_prices(self, double raw_bid_price, double raw_ask_price, uint64_t ts_event):
        # Apply tick scheme if available
        if self._spread_instrument._tick_scheme is not None:
            if raw_bid_price >= 0.:
                bid_price = self._spread_instrument._tick_scheme.next_bid_price(raw_bid_price)
            else:
                bid_price = self._spread_instrument.make_price(-self._spread_instrument._tick_scheme.next_ask_price(-raw_bid_price).as_double())

            if raw_ask_price >= 0.:
                ask_price = self._spread_instrument._tick_scheme.next_ask_price(raw_ask_price)
            else:
                ask_price = self._spread_instrument.make_price(-self._spread_instrument._tick_scheme.next_bid_price(-raw_ask_price).as_double())

            self._log.debug(f"Bid ask created using tick_scheme: {bid_price=}, {ask_price=}, {raw_bid_price=}, {raw_ask_price=}, is_futures_spread={self._is_futures_spread}")
        else:
            # Fallback to simple method if no tick scheme
            bid_price = self._spread_instrument.make_price(raw_bid_price)
            ask_price = self._spread_instrument.make_price(raw_ask_price)
            self._log.debug(f"Bid ask created: {bid_price=}, {ask_price=}, {raw_bid_price=}, {raw_ask_price=}, is_futures_spread={self._is_futures_spread}")

        # Create bid and ask sizes (use minimum of component sizes based on ratio signs)
        cdef double min_bid_size = float('inf')
        cdef double min_ask_size = float('inf')

        cdef int i
        for i in range(self._n_components):
            if self._ratios[i] >= 0:
                if self._bid_sizes[i] < min_bid_size:
                    min_bid_size = self._bid_sizes[i]

                if self._ask_sizes[i] < min_ask_size:
                    min_ask_size = self._ask_sizes[i]
            else:
                if self._ask_sizes[i] < min_bid_size:
                    min_bid_size = self._ask_sizes[i]

                if self._bid_sizes[i] < min_ask_size:
                    min_ask_size = self._bid_sizes[i]

        bid_size = self._spread_instrument.make_qty(min_bid_size)
        ask_size = self._spread_instrument.make_qty(min_ask_size)

        cdef QuoteTick spread_quote = QuoteTick(
            instrument_id=self._spread_instrument_id,
            bid_price=bid_price,
            ask_price=ask_price,
            bid_size=bid_size,
            ask_size=ask_size,
            ts_event=ts_event,
            ts_init=ts_event,
        )

        return spread_quote
