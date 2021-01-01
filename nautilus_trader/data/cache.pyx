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

"""
The `DataCache` provides an interface for consuming cached market data.
"""

from collections import deque
from decimal import Decimal

from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.data.base cimport DataCacheFacade
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick
from nautilus_trader.trading.calculators cimport ExchangeRateCalculator


cdef class DataCache(DataCacheFacade):
    """
    Provides a cache for the `DataEngine`.
    """

    def __init__(self, Logger logger not None, dict config=None):
        """
        Initialize a new instance of the `DataEngine` class.

        Parameters
        ----------
        logger : Logger
            The logger for the component.
        config : dict, optional
            The configuration options.

        """
        if config is None:
            config = {}

        self._log = LoggerAdapter(type(self).__name__, logger)
        self._xrate_calculator = ExchangeRateCalculator()
        self._xrate_symbols = {}

        # Capacities
        self.tick_capacity = config.get("tick_capacity", 1000)  # Per symbol
        self.bar_capacity = config.get("bar_capacity", 1000)    # Per symbol
        Condition.positive_int(self.tick_capacity, "tick_capacity")
        Condition.positive_int(self.bar_capacity, "bar_capacity")

        # Cached data
        self._instruments = {}  # type: dict[Symbol, Instrument]
        self._quote_ticks = {}  # type: dict[Symbol, list[QuoteTick]]
        self._trade_ticks = {}  # type: dict[Symbol, list[TradeTick]]
        self._bars = {}         # type: dict[BarType, list[Bar]]

        self._log.info("Initialized.")

# -- COMMANDS ---------------------------------------------------------------------------------------

    cpdef void reset(self) except *:
        """
        Reset the cache.

        All stateful fields are reset to their initial value.
        """
        self._log.info("Resetting cache...")

        self._xrate_symbols.clear()
        self._instruments.clear()
        self._quote_ticks.clear()
        self._trade_ticks.clear()
        self._bars.clear()

    cpdef void add_instrument(self, Instrument instrument) except *:
        """
        Add the given instrument to the cache.

        Parameters
        ----------
        instrument : Instrument
            The received instrument to add.

        """
        self._instruments[instrument.symbol] = instrument

        if self._is_crypto_spot_or_swap(instrument) or self._is_fx_spot(instrument):
            self._xrate_symbols[instrument.symbol] = (f"{instrument.base_currency}/"
                                                      f"{instrument.quote_currency}")

        self._log.debug(f"Updated instrument {instrument.symbol}")

    cpdef void add_quote_tick(self, QuoteTick tick) except *:
        """
        Add the given tick to the cache.

        Parameters
        ----------
        tick : QuoteTick
            The tick to handle.

        """
        Condition.not_none(tick, "tick")

        cdef Symbol symbol = tick.symbol
        ticks = self._quote_ticks.get(symbol)

        if ticks is None:
            # The symbol was not registered
            ticks = deque(maxlen=self.tick_capacity)
            self._quote_ticks[symbol] = ticks

        ticks.appendleft(tick)

    cpdef void add_trade_tick(self, TradeTick tick) except *:
        """
        Add the given tick to the cache.

        Parameters
        ----------
        tick : TradeTick
            The received tick to handle.

        """
        Condition.not_none(tick, "tick")

        cdef Symbol symbol = tick.symbol
        ticks = self._trade_ticks.get(symbol)

        if ticks is None:
            # The symbol was not registered
            ticks = deque(maxlen=self.tick_capacity)
            self._trade_ticks[symbol] = ticks

        ticks.appendleft(tick)

    cpdef void add_bar(self, BarType bar_type, Bar bar) except *:
        """
        Add the given bar type and bar to the cache.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the received bar.
        bar : Bar
            The received bar to handle.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(bar, "bar")

        # Update ticks
        bars = self._bars.get(bar_type)

        if bars is None:
            # The bar type was not registered
            bars = deque(maxlen=self.bar_capacity)
            self._bars[bar_type] = bars

        bars.appendleft(bar)

    cpdef void add_quote_ticks(self, list ticks) except *:
        """
        Add the given ticks to the cache, if it is empty.

        Parameters
        ----------
        ticks : list[QuoteTick]
            The tick to handle.

        """
        Condition.not_none(ticks, "ticks")

        cdef int length = len(ticks)
        cdef Symbol symbol
        if length > 0:
            symbol = ticks[0].symbol
            self._log.debug(f"Received <QuoteTick[{length}]> data for {symbol}.")
        else:
            self._log.debug("Received <QuoteTick[]> data with no ticks.")
            return

        cached_ticks = self._quote_ticks.get(symbol)

        if cached_ticks is None:
            # The symbol was not registered
            cached_ticks = deque(maxlen=self.tick_capacity)
            self._quote_ticks[symbol] = cached_ticks
        elif len(cached_ticks) > 0:
            # Currently the simple solution for multiple consumers requesting
            # ticks at system spool up; is just to add only if the cache is empty.
            self._log.debug("Cache already contains ticks.")
            return

        cdef QuoteTick tick
        for tick in ticks:
            cached_ticks.appendleft(tick)

    cpdef void add_trade_ticks(self, list ticks) except *:
        """
        Add the given ticks to the cache, if it is empty.

        Parameters
        ----------
        ticks : list[TradeTick]
            The received tick to handle.

        """
        Condition.not_none(ticks, "ticks")

        cdef int length = len(ticks)
        cdef Symbol symbol
        if length > 0:
            symbol = ticks[0].symbol
            self._log.debug(f"Received <TradeTick[{length}]> data for {symbol}.")
        else:
            self._log.debug("Received <TradeTick[]> data with no ticks.")
            return

        cached_ticks = self._trade_ticks.get(symbol)

        if cached_ticks is None:
            # The symbol was not registered
            cached_ticks = deque(maxlen=self.tick_capacity)
            self._trade_ticks[symbol] = cached_ticks
        elif len(cached_ticks) > 0:
            # Currently the simple solution for multiple consumers requesting
            # ticks at system spool up; is just to add only if the cache is empty.
            self._log.debug("Cache already contains ticks.")
            return

        cdef TradeTick tick
        for tick in ticks:
            cached_ticks.appendleft(tick)

    cpdef void add_bars(self, BarType bar_type, list bars) except *:
        """
        Handle the given bar type and bar.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the received bar.
        bars : list[Bar]
            The received bar to handle.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(bars, "bars")

        cdef int length = len(bars)
        if length > 0:
            self._log.debug(f"Received <Bar[{length}]> data for {bar_type.symbol}.")
        else:
            self._log.debug("Received <Bar[]> data with no ticks.")
            return

        cached_bars = self._trade_ticks.get(bar_type.symbol)

        if cached_bars is None:
            # The symbol was not registered
            cached_bars = deque(maxlen=self.bar_capacity)
            self._trade_ticks[bar_type.symbol] = cached_bars
        elif len(cached_bars) > 0:
            # Currently the simple solution for multiple consumers requesting
            # bars at system spool up; is just to add only if the cache is empty.
            self._log.debug("Cache already contains bars.")
            return

        cdef Bar bar
        for bar in bars:
            cached_bars.appendleft(bar)

# -- QUERIES ---------------------------------------------------------------------------------------

    cpdef list symbols(self):
        """
        Return all instrument symbols held by the data cache.

        Returns
        -------
        list[Symbol]
        """
        return sorted(list(self._instruments.keys()))

    cpdef list instruments(self):
        """
        Return all instruments held by the data cache.

        Returns
        -------
        list[Instrument]

        """
        return list(self._instruments.values())

    cpdef list quote_ticks(self, Symbol symbol):
        """
        Return the quote ticks for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the ticks to get.

        Returns
        -------
        list[QuoteTick]

        """
        Condition.not_none(symbol, "symbol")

        return list(self._quote_ticks.get(symbol, []))

    cpdef list trade_ticks(self, Symbol symbol):
        """
        Return trade ticks for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the ticks to get.

        Returns
        -------
        list[TradeTick]

        """
        Condition.not_none(symbol, "symbol")

        return list(self._trade_ticks.get(symbol, []))

    cpdef list bars(self, BarType bar_type):
        """
        Return bars for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to get.

        Returns
        -------
        list[Bar]

        """
        Condition.not_none(bar_type, "bar_type")

        return list(self._bars.get(bar_type, []))

    cpdef Instrument instrument(self, Symbol symbol):
        """
        Return the instrument corresponding to the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The symbol of the instrument to return.

        Returns
        -------
        Instrument or None

        """
        Condition.not_none(symbol, "symbol")

        return self._instruments.get(symbol)

    cpdef Price price(self, Symbol symbol, PriceType price_type):
        """
        Return the price for the given symbol and price type.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the price.
        price_type : PriceType
            The price type for the query.

        Returns
        -------
        Price or None

        Raises
        ------
        ValueError
            If price_type is UNDEFINED.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_equal(price_type, PriceType.UNDEFINED, "price_type", "UNDEFINED")

        cdef TradeTick trade_tick
        cdef QuoteTick quote_tick

        if price_type == PriceType.LAST:
            trade_tick = self.trade_tick(symbol)
            return trade_tick.price if trade_tick is not None else None
        else:
            quote_tick = self.quote_tick(symbol)
            return quote_tick.extract_price(price_type) if quote_tick is not None else None

    cpdef QuoteTick quote_tick(self, Symbol symbol, int index=0):
        """
        Return the quote tick for the given symbol at the given index.

        Last quote tick if no index specified.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the tick to get.
        index : int, optional
            The index for the tick to get.

        Returns
        -------
        QuoteTick or None
            If no ticks or no tick at index then returns None.

        Notes
        -----
        Reverse indexed (most recent tick at index 0).

        """
        Condition.not_none(symbol, "symbol")

        ticks = self._quote_ticks.get(symbol)
        if not ticks:
            return None

        try:
            return ticks[index]
        except IndexError:
            return None

    cpdef TradeTick trade_tick(self, Symbol symbol, int index=0):
        """
        Return the trade tick for the given symbol at the given index

        Last trade tick if no index specified.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the tick to get.
        index : int, optional
            The index for the tick to get.

        Returns
        -------
        TradeTick or None
            If no ticks or no tick at index then returns None.

        Notes
        -----
        Reverse indexed (most recent tick at index 0).

        """
        Condition.not_none(symbol, "symbol")

        ticks = self._trade_ticks.get(symbol)
        if not ticks:
            return None

        try:
            return ticks[index]
        except IndexError:
            return None

    cpdef Bar bar(self, BarType bar_type, int index=0):
        """
        Return the bar for the given bar type at the given index.

        Last bar if no index specified.

        Parameters
        ----------
        bar_type : BarType
            The bar type to get.
        index : int, optional
            The index for the bar to get.

        Returns
        -------
        Bar or None
            If no bars or no bar at index then returns None.

        Notes
        -----
        Reverse indexed (most recent bar at index 0).

        """
        Condition.not_none(bar_type, "bar_type")

        bars = self._bars.get(bar_type)
        if not bars:
            return None

        try:
            return bars[index]
        except IndexError:
            return None

    cpdef int quote_tick_count(self, Symbol symbol) except *:
        """
        The count of quote ticks for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the ticks.

        Returns
        -------
        int

        """
        Condition.not_none(symbol, "symbol")

        return len(self._quote_ticks.get(symbol, []))

    cpdef int trade_tick_count(self, Symbol symbol) except *:
        """
        The count of trade ticks for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the ticks.

        Returns
        -------
        int

        """
        Condition.not_none(symbol, "symbol")

        return len(self._trade_ticks.get(symbol, []))

    cpdef int bar_count(self, BarType bar_type) except *:
        """
        The count of bars for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to count.

        Returns
        -------
        int

        """
        Condition.not_none(bar_type, "bar_type")

        return len(self._bars.get(bar_type, []))

    cpdef bint has_quote_ticks(self, Symbol symbol) except *:
        """
        Return a value indicating whether the data engine has quote ticks for
        the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the ticks.

        Returns
        -------
        bool

        """
        Condition.not_none(symbol, "symbol")

        return self.quote_tick_count(symbol) > 0

    cpdef bint has_trade_ticks(self, Symbol symbol) except *:
        """
        Return a value indicating whether the data engine has trade ticks for
        the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the ticks.

        Returns
        -------
        bool

        """
        Condition.not_none(symbol, "symbol")

        return self.trade_tick_count(symbol) > 0

    cpdef bint has_bars(self, BarType bar_type) except *:
        """
        Return a value indicating whether the data engine has bars for the given
        bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the bars.

        Returns
        -------
        bool

        """
        Condition.not_none(bar_type, "bar_type")

        return self.bar_count(bar_type) > 0

    cpdef object get_xrate(
        self,
        Venue venue,
        Currency from_currency,
        Currency to_currency,
        PriceType price_type=PriceType.MID,
    ):
        """
        Return the calculated exchange rate for the given currencies.

        Parameters
        ----------
        venue : Venue
            The venue for the exchange rate.
        from_currency : Currency
            The currency to convert from.
        to_currency : Currency
            The currency to convert to.
        price_type : PriceType
            The price type for the exchange rate.

        Returns
        -------
        Decimal

        Raises
        ------
        ValueError
            If price_type is UNDEFINED or LAST.

        """
        Condition.not_none(from_currency, "from_currency")
        Condition.not_none(to_currency, "to_currency")

        if from_currency == to_currency:
            return Decimal(1)  # No conversion necessary

        cdef tuple quotes = self._build_quote_table(venue)

        return self._xrate_calculator.get_rate(
            from_currency=from_currency,
            to_currency=to_currency,
            price_type=price_type,
            bid_quotes=quotes[0],  # Bid
            ask_quotes=quotes[1],  # Ask
        )

    cdef inline tuple _build_quote_table(self, Venue venue):
        cdef dict bid_quotes = {}
        cdef dict ask_quotes = {}

        cdef Symbol symbol
        cdef str base_quote
        for symbol, base_quote in self._xrate_symbols.items():
            if symbol.venue != venue:
                continue

            ticks = self._quote_ticks.get(symbol)
            if not ticks:
                # No quotes for symbol
                continue

            bid_quotes[base_quote] = ticks[0].bid.as_decimal()
            ask_quotes[base_quote] = ticks[0].ask.as_decimal()

        return bid_quotes, ask_quotes

    cdef inline bint _is_crypto_spot_or_swap(self, Instrument instrument) except *:
        return instrument.asset_class == AssetClass.CRYPTO \
            and (instrument.asset_type == AssetType.SPOT or instrument.asset_type == AssetType.SWAP)

    cdef inline bint _is_fx_spot(self, Instrument instrument) except *:
        return instrument.asset_class == AssetClass.FX and instrument.asset_type == AssetType.SPOT
