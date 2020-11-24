# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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
This module provides components relating to data for backtesting.

A `BacktestDataContainer` is a convenient container for holding and organizing
backtest related data - which can be passed to one or more `BacktestDataEngine`(s).
"""

import gc

import numpy as np
import pandas as pd

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta

from pandas import DatetimeIndex

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport format_bytes
from nautilus_trader.core.functions cimport get_size_of
from nautilus_trader.core.functions cimport slice_dataframe
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.data.wrangling cimport QuoteTickDataWrangler
from nautilus_trader.data.wrangling cimport TradeTickDataWrangler
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregationParser
from nautilus_trader.model.c_enums.maker cimport MakerParser
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.identifiers cimport TradeMatchId
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.tick cimport QuoteTick


cdef class BacktestDataContainer:
    """
    Provides a container for backtest data.
    """

    def __init__(self):
        """
        Initialize a new instance of the `BacktestDataContainer` class.
        """
        self.symbols = set()   # type: {Instrument}
        self.instruments = {}  # type: {Symbol, Instrument}
        self.quote_ticks = {}  # type: {Symbol, pd.DataFrame}
        self.trade_ticks = {}  # type: {Symbol, pd.DataFrame}
        self.bars_bid = {}     # type: {Symbol, {BarAggregation, pd.DataFrame}}
        self.bars_ask = {}     # type: {Symbol, {BarAggregation, pd.DataFrame}}

    cpdef void add_instrument(self, Instrument instrument) except *:
        """
        Add the instrument to the container.

        Parameters
        ----------
        instrument : Instrument
            The instrument to add.

        """
        Condition.not_none(instrument, "instrument")

        self.instruments[instrument.symbol] = instrument
        self.instruments = dict(sorted(self.instruments.items()))

    cpdef void add_quote_ticks(self, Symbol symbol, data: pd.DataFrame) except *:
        """
        Add the quote tick data to the container.

        The format of the dataframe is expected to be a DateTimeIndex (times are
        assumed to be UTC, and are converted to tz-aware in pre-processing).

        With index column named 'timestamp', and 'bid', 'ask' data columns.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the quote tick data.
        data : pd.DataFrame
            The quote tick data to add.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(data, "data")
        Condition.type(data, pd.DataFrame, "data")

        self.symbols.add(symbol)
        self.quote_ticks[symbol] = data
        self.quote_ticks = dict(sorted(self.quote_ticks.items()))

    cpdef void add_trade_ticks(self, Symbol symbol, data: pd.DataFrame) except *:
        """
        Add the trade tick data to the container.

        The format of the dataframe is expected to be a DateTimeIndex (times are
        assumed to be UTC, and are converted to tz-aware in pre-processing).

        With index column named 'timestamp', and 'trade_id', 'price', 'quantity',
        'buyer_maker' data columns.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the trade tick data.
        data : pd.DataFrame
            The trade tick data to add.

        Returns
        -------

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(data, "data")
        Condition.type(data, pd.DataFrame, "data")

        self.symbols.add(symbol)
        self.trade_ticks[symbol] = data
        self.trade_ticks = dict(sorted(self.trade_ticks.items()))

    cpdef void add_bars(
            self,
            Symbol symbol,
            BarAggregation aggregation,
            PriceType price_type,
            data: pd.DataFrame
    ) except *:
        """
        Add the bar data to the container.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the bar data.
        aggregation : BarAggregation
            The bar aggregation of the data.
        price_type : PriceType
            The price type of the data.
        data : pd.DataFrame
            The bar data to add.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(data, "data")
        Condition.true(price_type != PriceType.LAST, "price_type != PriceType.LAST")

        self.symbols.add(symbol)

        if price_type == PriceType.BID:
            if symbol not in self.bars_bid:
                self.bars_bid[symbol] = {}
                self.bars_bid = dict(sorted(self.bars_bid.items()))
            self.bars_bid[symbol][aggregation] = data
            self.bars_bid[symbol] = dict(sorted(self.bars_bid[symbol].items()))

        if price_type == PriceType.ASK:
            if symbol not in self.bars_ask:
                self.bars_ask[symbol] = {}
                self.bars_ask = dict(sorted(self.bars_ask.items()))
            self.bars_ask[symbol][aggregation] = data
            self.bars_ask[symbol] = dict(sorted(self.bars_ask[symbol].items()))

    cpdef void check_integrity(self) except *:
        """
        Check the integrity of the data inside the container.

        Raises
        ------
        ValueError
            If any integrity check fails.

        """
        # Check there is the needed instrument for each data symbol
        for symbol in self.symbols:
            Condition.true(symbol in self.instruments, f"symbol in self.instruments")

        # Check that all bar DataFrames for each symbol are of the same shape and index
        cdef dict shapes = {}  # type: {BarAggregation, tuple}
        cdef dict indexs = {}  # type: {BarAggregation, DatetimeIndex}
        for symbol, data in self.bars_bid.items():
            for aggregation, dataframe in data.items():
                if aggregation not in shapes:
                    shapes[aggregation] = dataframe.shape
                if aggregation not in indexs:
                    indexs[aggregation] = dataframe.index
                if dataframe.shape != shapes[aggregation]:
                    raise RuntimeError(f"{dataframe} bid ask shape is not equal.")
                if not all(dataframe.index == indexs[aggregation]):
                    raise RuntimeError(f"{dataframe} bid ask index is not equal.")
        for symbol, data in self.bars_ask.items():
            for aggregation, dataframe in data.items():
                if dataframe.shape != shapes[aggregation]:
                    raise RuntimeError(f"{dataframe} bid ask shape is not equal.")
                if not all(dataframe.index == indexs[aggregation]):
                    raise RuntimeError(f"{dataframe} bid ask index is not equal.")

    cpdef bint has_quote_data(self, Symbol symbol) except *:
        """
        If the container has quote data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The query symbol.

        Returns
        -------
        bool

        """
        Condition.not_none(symbol, "symbol")
        return symbol in self.quote_ticks or symbol in self.bars_bid

    cpdef bint has_trade_data(self, Symbol symbol) except *:
        """
        If the container has trade data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The query symbol.

        Returns
        -------
        bool

        """
        Condition.not_none(symbol, "symbol")
        return symbol in self.trade_ticks

    cpdef long total_data_size(self):
        """
        Return the total memory size of the data in the container.

        Returns
        -------
        long
            The total bytes.

        """
        cdef long size = 0
        size += get_size_of(self.quote_ticks)
        size += get_size_of(self.trade_ticks)
        size += get_size_of(self.bars_bid)
        size += get_size_of(self.bars_ask)
        return size


cdef class BacktestDataProducer(DataClient):
    """
    Provides an implementation of `DataClient` which produces data for backtesting.
    """

    def __init__(
            self,
            BacktestDataContainer data not None,
            Venue venue not None,
            DataEngine engine not None,
            Clock clock not None,
            Logger logger not None,
    ):
        """
        Initialize a new instance of the `BacktestDataProducer` class.

        venue : Venue
            The venue the producer provides data for.
        engine : DataEngine
            The data engine to connect to the producer.
        clock : Clock
            The clock for the component.
        logger : Logger
            The logger for the component.

        """
        super().__init__(
            venue,
            engine,
            clock,
            logger,
        )

        self._is_connected = False

        # Check data integrity
        data.check_integrity()
        self._data = data

        cdef int symbol_counter = 0
        self._symbol_index = {}

        # Prepare instruments
        for instrument in self._data.instruments.values():
            self._engine.process(instrument)

        # Prepare data
        self._quote_tick_data = pd.DataFrame()
        self._trade_tick_data = pd.DataFrame()
        cdef list quote_tick_frames = []
        cdef list trade_tick_frames = []
        self.execution_resolutions = []

        timing_start_total = datetime.utcnow()
        for instrument in data.instruments.values():
            symbol = instrument.symbol
            self._log.info(f"Preparing {symbol} data...")

            self._symbol_index[symbol_counter] = symbol

            execution_resolution = None

            # Process quote tick data
            # -----------------------
            if data.has_quote_data(symbol):
                timing_start = datetime.utcnow()  # Time data processing
                quote_wrangler = QuoteTickDataWrangler(
                    instrument=instrument,
                    data_quotes=self._data.quote_ticks.get(symbol),
                    data_bars_bid=self._data.bars_bid.get(symbol),
                    data_bars_ask=self._data.bars_ask.get(symbol),
                )

                # noinspection PyUnresolvedReferences
                quote_wrangler.pre_process(symbol_counter)
                quote_tick_frames.append(quote_wrangler.processed_data)

                execution_resolution = BarAggregationParser.to_string(quote_wrangler.resolution)
                self._log.info(f"Prepared {len(quote_wrangler.processed_data):,} {symbol} quote tick rows in "
                               f"{round((datetime.utcnow() - timing_start).total_seconds(), 2)}s.")
                del quote_wrangler  # Dump processing artifact

            # Process trade tick data
            # -----------------------
            if data.has_trade_data(symbol):
                timing_start = datetime.utcnow()  # Time data processing
                trade_wrangler = TradeTickDataWrangler(
                    instrument=instrument,
                    data=self._data.trade_ticks.get(symbol),
                )

                # noinspection PyUnresolvedReferences
                trade_wrangler.pre_process(symbol_counter)
                trade_tick_frames.append(trade_wrangler.processed_data)

                execution_resolution = BarAggregationParser.to_string(BarAggregation.TICK)
                self._log.info(f"Prepared {len(trade_wrangler.processed_data):,} {symbol} trade tick rows in "
                               f"{round((datetime.utcnow() - timing_start).total_seconds(), 2)}s.")
                del trade_wrangler  # Dump processing artifact

            if execution_resolution is None:
                self._log.warning(f"No execution level data for {symbol}.")

            # Increment counter for indexing the next symbol
            symbol_counter += 1

            self.execution_resolutions.append(f"{symbol}={execution_resolution}")

        # Merge and sort all ticks
        self._log.info(f"Merging tick data streams...")
        if quote_tick_frames:
            self._quote_tick_data = pd.concat(quote_tick_frames)
            self._quote_tick_data.sort_index(axis=0, kind="mergesort", inplace=True)

        if trade_tick_frames:
            self._trade_tick_data = pd.concat(trade_tick_frames)
            self._trade_tick_data.sort_index(axis=0, kind="mergesort", inplace=True)

        # Set min and max timestamps
        self.min_timestamp = None
        self.max_timestamp = None

        if not self._quote_tick_data.empty:
            self.min_timestamp = self._quote_tick_data.index.min()
            self.max_timestamp = self._quote_tick_data.index.max()

        if not self._trade_tick_data.empty:
            if self.min_timestamp is None:
                self.min_timestamp = self._trade_tick_data.index.min()
            else:
                self.min_timestamp = max(self._quote_tick_data.index, self._trade_tick_data.index)

            if self.max_timestamp is None:
                self.max_timestamp = self._trade_tick_data.index.max()
            else:
                self.max_timestamp = min(self._quote_tick_data.index, self._trade_tick_data.index)

        # Initialize backing fields
        self._quote_symbols = None
        self._quote_bids = None
        self._quote_asks = None
        self._quote_bid_sizes = None
        self._quote_ask_sizes = None
        self._quote_timestamps = None
        self._quote_index = 0
        self._quote_index_last = 0
        self._next_quote_tick = None

        self._trade_symbols = None
        self._trade_prices = None
        self._trade_sizes = None
        self._trade_match_ids = None
        self._trade_makers = None
        self._trade_timestamps = None
        self._trade_index = 0
        self._trade_index_last = 0
        self._next_trade_tick = None

        self.has_tick_data = False

        processing_time = round((datetime.utcnow() - timing_start_total).total_seconds(), 2)
        self._log.info(f"Prepared {len(self._quote_tick_data) + len(self._trade_tick_data):,} "
                       f"total tick rows in {processing_time}s.")

        gc.collect()  # Garbage collection to remove redundant processing artifacts

    cpdef void setup(self, datetime start, datetime stop) except *:
        """
        Setup tick data for a backtest run.

        Parameters
        ----------
        start : datetime
            The start datetime (UTC) for the run.
        stop : datetime
            The stop datetime (UTC) for the run.

        """
        Condition.not_none(start, "start")
        Condition.not_none(stop, "stop")

        # Prepare instruments
        for instrument in self._data.instruments.values():
            self._engine.process(instrument)

        # Calculate data size
        cdef long total_size = 0

        # Build quote tick data stream
        if not self._quote_tick_data.empty:
            time_buffer = timedelta(milliseconds=1)  # To ensure we don't pickup an `unwanted` generated tick
            # See slice_dataframe function comments on why [:] isn't used
            quote_ticks_slice = slice_dataframe(self._quote_tick_data, start + time_buffer, stop)

            self._quote_symbols = quote_ticks_slice["symbol"].to_numpy(dtype=np.ushort)
            self._quote_bids = quote_ticks_slice["bid"].values
            self._quote_asks = quote_ticks_slice["ask"].values
            self._quote_bid_sizes = quote_ticks_slice["bid_size"].values
            self._quote_ask_sizes = quote_ticks_slice["ask_size"].values
            self._quote_timestamps = np.asarray([<datetime>dt for dt in quote_ticks_slice.index])

            # Calculate cumulative data size
            total_size += get_size_of(self._quote_symbols)
            total_size += get_size_of(self._quote_bids)
            total_size += get_size_of(self._quote_asks)
            total_size += get_size_of(self._quote_bid_sizes)
            total_size += get_size_of(self._quote_ask_sizes)
            total_size += get_size_of(self._quote_timestamps)

            # Set indexing
            self._quote_index = 0
            self._quote_index_last = len(quote_ticks_slice) - 1

            # Prepare initial tick
            self._iterate_quote_ticks()

        # Build trade tick data stream
        if not self._trade_tick_data.empty:
            # See slice_dataframe function comments on why [:] isn't used
            trade_ticks_slice = slice_dataframe(self._trade_tick_data, start, stop)

            self._trade_symbols = trade_ticks_slice["symbol"].to_numpy(dtype=np.ushort)
            self._trade_prices = trade_ticks_slice["price"].values
            self._trade_sizes = trade_ticks_slice["quantity"].values
            self._trade_match_ids = trade_ticks_slice["match_id"].values
            self._trade_makers = trade_ticks_slice["buyer_maker"].values
            self._trade_timestamps = np.asarray([<datetime>dt for dt in trade_ticks_slice.index])

            # Calculate cumulative data size
            total_size += get_size_of(self._trade_symbols)
            total_size += get_size_of(self._trade_prices)
            total_size += get_size_of(self._trade_sizes)
            total_size += get_size_of(self._trade_match_ids)
            total_size += get_size_of(self._trade_makers)
            total_size += get_size_of(self._trade_timestamps)

            # Set indexing
            self._trade_index = 0
            self._trade_index_last = len(trade_ticks_slice) - 1

            # Prepare initial tick
            self._iterate_trade_ticks()

        self.has_tick_data = True

        self._log.info(f"Data stream size: {format_bytes(total_size)}")

    cdef Tick next_tick(self):
        cdef Tick next_tick
        # Quote ticks only
        if self._next_trade_tick is None:
            next_tick = self._next_quote_tick
            self._iterate_quote_ticks()
            return next_tick
        # Trade ticks only
        if self._next_quote_tick is None:
            next_tick = self._next_trade_tick
            self._iterate_trade_ticks()
            return next_tick

        # Mixture of quote and trade ticks
        if self._next_quote_tick.timestamp <= self._next_trade_tick.timestamp:
            next_tick = self._next_quote_tick
            self._iterate_quote_ticks()
            return next_tick
        else:
            next_tick = self._next_trade_tick
            self._iterate_trade_ticks()
            return next_tick

    cdef inline QuoteTick _generate_quote_tick(self, int index):
        return QuoteTick(
            self._symbol_index[self._quote_symbols[index]],
            Price(self._quote_bids[index]),
            Price(self._quote_asks[index]),
            Quantity(self._quote_bid_sizes[index]),
            Quantity(self._quote_ask_sizes[index]),
            self._quote_timestamps[index],
        )

    cdef inline TradeTick _generate_trade_tick(self, int index):
        return TradeTick(
            self._symbol_index[self._trade_symbols[index]],
            Price(self._trade_prices[index]),
            Quantity(self._trade_sizes[index]),
            MakerParser.from_string(self._trade_makers[index]),
            TradeMatchId(self._trade_match_ids[index]),
            self._trade_timestamps[index],
        )

    cdef inline void _iterate_quote_ticks(self) except *:
        if self._quote_index <= self._quote_index_last:
            self._next_quote_tick = self._generate_quote_tick(self._quote_index)
            self._quote_index += 1
        else:
            self._next_quote_tick = None
            if self._next_trade_tick is None:
                self.has_tick_data = False

    cdef inline void _iterate_trade_ticks(self) except *:
        if self._trade_index <= self._trade_index_last:
            self._next_trade_tick = self._generate_trade_tick(self._trade_index)
            self._trade_index += 1
        else:
            self._next_trade_tick = None
            if self._next_quote_tick is None:
                self.has_tick_data = False

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef bint is_connected(self) except *:
        """
        Return a value indicating whether the client is connected.

        Returns
        -------
        bool
            True if connected, else False.

        """
        return self._is_connected

    cpdef void connect(self) except *:
        """
        Connect the client.
        """
        self._is_connected = True
        self._log.debug(f"Connected.")

    cpdef void disconnect(self) except *:
        """
        Disconnect the client.
        """
        self._is_connected = False
        self._log.debug(f"Disconnected.")

    cpdef void reset(self) except *:
        """
        Reset the data client.

        All stateful values are reset to their initial value.
        """
        self._log.debug(f"Resetting...")

        self._quote_symbols = None
        self._quote_bids = None
        self._quote_asks = None
        self._quote_bid_sizes = None
        self._quote_ask_sizes = None
        self._quote_timestamps = None
        self._quote_index = 0
        self._quote_index_last = len(self._quote_tick_data) - 1

        self._trade_symbols = None
        self._trade_prices = None
        self._trade_sizes = None
        self._trade_match_ids = None
        self._trade_makers = None
        self._trade_timestamps = None
        self._trade_index = 0
        self._trade_index_last = len(self._quote_tick_data) - 1

        self.has_tick_data = False

        self._log.info("Reset.")

    cpdef void dispose(self) except *:
        """
        Dispose of the data client.

        This method is idempotent and irreversible. No other methods should be
        called after disposal.
        """
        pass  # Nothing to dispose

# -- REQUESTS --------------------------------------------------------------------------------------

    cpdef void request_quote_ticks(
            self,
            Symbol symbol,
            datetime from_datetime,  # Can be None
            datetime to_datetime,    # Can be None
            int limit,
            UUID correlation_id,
    ) except *:
        Condition.not_none(symbol, "symbol")
        Condition.not_negative_int(limit, "limit")
        Condition.not_none(correlation_id, "correlation_id")

        if not self._is_connected:
            self._log.error(f"Cannot request quote ticks for {symbol} (not connected).")
            return

        # Do nothing else for backtest

    cpdef void request_trade_ticks(
            self,
            Symbol symbol,
            datetime from_datetime,  # Can be None
            datetime to_datetime,    # Can be None
            int limit,
            UUID correlation_id,
    ) except *:
        Condition.not_none(symbol, "symbol")
        Condition.not_negative_int(limit, "limit")
        Condition.not_none(correlation_id, "correlation_id")

        if not self._is_connected:
            self._log.error(f"Cannot request trade ticks for {symbol} (not connected).")
            return

        # Do nothing else for backtest

    cpdef void request_bars(
            self,
            BarType bar_type,
            datetime from_datetime,  # Can be None
            datetime to_datetime,    # Can be None
            int limit,
            UUID correlation_id,
    ) except *:
        Condition.not_none(bar_type, "bar_type")
        Condition.not_negative_int(limit, "limit")
        Condition.not_none(correlation_id, "correlation_id")

        if not self._is_connected:
            self._log.error(f"Cannot request bars for {bar_type} (not connected).")
            return

        # Do nothing else for backtest

    cpdef void request_instrument(self, Symbol symbol, UUID correlation_id) except *:
        Condition.not_none(symbol, "symbol")
        Condition.not_none(correlation_id, "correlation_id")

        if not self._is_connected:
            self._log.error(f"Cannot request instrument for {symbol} (not connected).")
            return

        cdef Instrument instrument = self._data.instruments.get(symbol)

        if instrument is None:
            self._log.warning(f"No instrument found for {symbol}.")
            return

        self._handle_instruments([instrument], correlation_id)

    cpdef void request_instruments(self, UUID correlation_id) except *:
        self._handle_instruments(list(self._data.instruments.values()), correlation_id)

# -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    cpdef void subscribe_instrument(self, Symbol symbol) except *:
        if not self._is_connected:
            self._log.error(f"Cannot subscribe to instrument for {symbol} (not connected).")
            return

        # Do nothing else for backtest

    cpdef void subscribe_quote_ticks(self, Symbol symbol) except *:
        if not self._is_connected:
            self._log.error(f"Cannot subscribe to quote ticks for {symbol} (not connected).")
            return

        # Do nothing else for backtest

    cpdef void subscribe_trade_ticks(self, Symbol symbol) except *:
        if not self._is_connected:
            self._log.error(f"Cannot subscribe to trade ticks for {symbol} (not connected).")
            return

        # Do nothing else for backtest

    cpdef void subscribe_bars(self, BarType bar_type) except *:
        if not self._is_connected:
            self._log.error(f"Cannot subscribe to bars for {bar_type} (not connected).")
            return

        self._log.error(f"Cannot subscribe to externally aggregated bars "
                        f"(backtesting only supports internal aggregation at this stage).")

    cpdef void unsubscribe_instrument(self, Symbol symbol) except *:
        if not self._is_connected:
            self._log.error(f"Cannot unsubscribe from instrument for {symbol} (not connected).")
            return

        # Do nothing else for backtest

    cpdef void unsubscribe_quote_ticks(self, Symbol symbol) except *:
        if not self._is_connected:
            self._log.error(f"Cannot unsubscribe from quote ticks for {symbol} (not connected).")
            return

        # Do nothing else for backtest

    cpdef void unsubscribe_trade_ticks(self, Symbol symbol) except *:
        if not self._is_connected:
            self._log.error(f"Cannot unsubscribe from trade ticks for {symbol} (not connected).")
            return

        # Do nothing else for backtest

    cpdef void unsubscribe_bars(self, BarType bar_type) except *:
        if not self._is_connected:
            self._log.error(f"Cannot unsubscribe from bars {bar_type} (not connected).")
            return

        self._log.error(f"Cannot unsubscribe from externally aggregated bars "
                        f"(backtesting only supports internal aggregation at this stage).")
