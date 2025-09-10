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

from datetime import timedelta

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.common.component cimport TimeEvent
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.greeks cimport GreeksCalculator
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument


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
    ):
        Condition.is_true(spread_instrument_id.is_spread(), "instrument_id must be a spread")

        super().__init__(clock=clock, msgbus=msgbus)
        self._handler = handler
        self._cache = cache
        self._log = Logger(name=f"{type(self).__name__}")

        self._spread_instrument_id = spread_instrument_id
        self._components = spread_instrument_id.to_list()
        self._greeks_calculator = GreeksCalculator(msgbus, cache, clock)

        self._component_ids = [component[0] for component in self._components]
        self._ratios = np.array([component[1] for component in self._components])
        n_components = len(self._components)
        self._mid_prices = np.zeros(n_components)
        self._vegas = np.zeros(n_components)
        self._bid_ask_spreads = np.zeros(n_components)
        self._bid_sizes = np.zeros(n_components)
        self._ask_sizes = np.zeros(n_components)

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
        # Get spread instrument to use its attributes
        cdef Instrument spread_instrument = self._cache.instrument(self._spread_instrument_id)

        if spread_instrument is None:
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

            greeks_data = self._greeks_calculator.instrument_greeks(
                component_id,
                percent_greeks=True,
                use_cached_greeks=True,  # Use cached greeks for testing
                vega_time_weight_base=30, # 30-day time weight base
            )

            if greeks_data is None:
                missing_greeks.append(str(component_id))
                continue

            ask_price = component_quote.ask_price.as_double()
            bid_price = component_quote.bid_price.as_double()

            self._mid_prices[i] = (ask_price + bid_price) * 0.5
            self._bid_ask_spreads[i] = ask_price - bid_price
            self._vegas[i] = greeks_data.vega
            self._bid_sizes[i] = component_quote.bid_size.as_double()
            self._ask_sizes[i] = component_quote.ask_size.as_double()

        # Check if we have all required data (use debug for timer-driven recurring conditions)
        if missing_quotes:
            self._log.debug(
                f"Missing quotes for spread {self._spread_instrument_id} components: {', '.join(missing_quotes)}"
            )
            return

        if missing_greeks:
            self._log.debug(
                f"Missing greeks for spread {self._spread_instrument_id} components: {', '.join(missing_greeks)}"
            )
            return

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
            self._log.debug(
                f"All vegas are zero for spread {self._spread_instrument_id}, cannot generate spread quote"
            )
            return

        vega_multiplier = np.abs(non_zero_multipliers).mean()
        spread_vega = abs(np.dot(self._vegas, self._ratios))

        bid_ask_spread = spread_vega * vega_multiplier
        self._log.debug(f"{self._bid_ask_spreads=}, {self._vegas=}, {vega_multipliers=}, "
                          f"{spread_vega=}, {vega_multiplier=}, {bid_ask_spread=}")

        # Calculate raw bid/ask prices
        spread_mid_price = (self._mid_prices * self._ratios).sum()
        raw_bid_price = spread_mid_price - bid_ask_spread * 0.5
        raw_ask_price = spread_mid_price + bid_ask_spread * 0.5

        if spread_instrument._tick_scheme is not None:
            if raw_bid_price >= 0.:
                bid_price = spread_instrument._tick_scheme.next_bid_price(raw_bid_price)
            else:
                bid_price = spread_instrument.make_price(-spread_instrument._tick_scheme.next_ask_price(-raw_bid_price).as_double())

            if raw_ask_price >= 0.:
                ask_price = spread_instrument._tick_scheme.next_ask_price(raw_ask_price)
            else:
                ask_price = spread_instrument.make_price(-spread_instrument._tick_scheme.next_bid_price(-raw_ask_price).as_double())

            self._log.debug(f"Bid ask created using tick_scheme: {bid_price=}, {ask_price=}, {raw_bid_price=}, {raw_ask_price=}")
        else:
            # Fallback to simple method if no tick scheme
            bid_price = spread_instrument.make_price(raw_bid_price)
            ask_price = spread_instrument.make_price(raw_ask_price)
            self._log.debug(f"Bid ask created: {bid_price=}, {ask_price=}, {raw_bid_price=}, {raw_ask_price=}")

        # Create bid and ask sizes
        bid_size = spread_instrument.make_qty(self._bid_sizes.min())
        ask_size = spread_instrument.make_qty(self._ask_sizes.min())

        cdef QuoteTick spread_quote = QuoteTick(
            instrument_id=self._spread_instrument_id,
            bid_price=bid_price,
            ask_price=ask_price,
            bid_size=bid_size,
            ask_size=ask_size,
            ts_event=event.ts_event,
            ts_init=event.ts_event,
        )

        # Send quote to the backtest engine so it reaches a venue and a matching engine
        self._msgbus.send(endpoint=f"SimulatedExchange.spread_quote.{self._spread_instrument_id.venue}", msg=spread_quote)

        # Send the spread quote to the data engine *after* it's possibly processed by a matching engine
        # like in the main backtesting loop in the backtest engine
        self._handler(spread_quote)
