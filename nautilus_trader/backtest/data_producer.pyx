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
This module provides a data producer for backtesting.
"""

import gc

import numpy as np
import pandas as pd
from bisect import bisect_left

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport format_bytes
from nautilus_trader.core.functions cimport get_size_of
from nautilus_trader.core.functions cimport slice_dataframe
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.data.wrangling cimport QuoteTickDataWrangler
from nautilus_trader.data.wrangling cimport TradeTickDataWrangler
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregationParser
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.identifiers cimport TradeMatchId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.tick cimport QuoteTick


cdef class DataProducerFacade:
    """
        Provides a read-only facade for a `DataProducerFacade`.
        """

    cpdef void setup(self, datetime start, datetime stop) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void reset(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Tick next_tick(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")


cdef class BacktestDataProducer(DataProducerFacade):
    """
    Provides an implementation of `DataClient` which produces data for backtesting.
    """

    def __init__(
        self,
        BacktestDataContainer data not None,
        DataEngine engine not None,
        Clock clock not None,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the `BacktestDataProducer` class.

        data : BacktestDataContainer
            The data for the producer.
        engine : DataEngine
            The data engine to connect to the producer.
        clock : Clock
            The clock for the component.
        logger : Logger
            The logger for the component.

        """
        self._clock = clock
        self._log = LoggerAdapter(type(self).__name__, logger)
        self._data_engine = engine

        # Check data integrity
        data.check_integrity()
        self._data = data

        cdef int symbol_counter = 0
        self._symbol_index = {}

        # Prepare instruments
        for instrument in self._data.instruments.values():
            self._data_engine.process(instrument)

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

                execution_resolution = BarAggregationParser.to_str(quote_wrangler.resolution)
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

                execution_resolution = BarAggregationParser.to_str(BarAggregation.TICK)
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
                self.min_timestamp = min(self._quote_tick_data.index.min(), self._trade_tick_data.index.min())

            if self.max_timestamp is None:
                self.max_timestamp = self._trade_tick_data.index.max()
            else:
                self.max_timestamp = max(self._quote_tick_data.index.max(), self._trade_tick_data.index.max())

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
        self._trade_sides = None
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
            self._data_engine.process(instrument)

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
            self._trade_sides = trade_ticks_slice["side"].values
            self._trade_timestamps = np.asarray([<datetime>dt for dt in trade_ticks_slice.index])

            # Calculate cumulative data size
            total_size += get_size_of(self._trade_symbols)
            total_size += get_size_of(self._trade_prices)
            total_size += get_size_of(self._trade_sizes)
            total_size += get_size_of(self._trade_match_ids)
            total_size += get_size_of(self._trade_sides)
            total_size += get_size_of(self._trade_timestamps)

            # Set indexing
            self._trade_index = 0
            self._trade_index_last = len(trade_ticks_slice) - 1

            # Prepare initial tick
            self._iterate_trade_ticks()

        self.has_tick_data = True

        self._log.info(f"Data stream size: {format_bytes(total_size)}")

    cpdef Tick next_tick(self):
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
            OrderSideParser.from_str(self._trade_sides[index]),
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

    cpdef void reset(self) except *:
        """
        Reset the data producer.

        All stateful fields are reset to their initial value.
        """
        self._log.info(f"Resetting...")

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
        self._trade_sides = None
        self._trade_timestamps = None
        self._trade_index = 0
        self._trade_index_last = len(self._quote_tick_data) - 1

        self.has_tick_data = False

        self._log.info("Reset.")

    cpdef void clear(self) except *:

        self.reset()
        self._trade_tick_data = pd.DataFrame()
        self._quote_tick_data = pd.DataFrame()

        gc.collect()  # Garbage collection to remove redundant processing artifacts
        self._log.info("Clear.")


cdef class CachedProducer(DataProducerFacade):

    def  __init__(
            self,
            BacktestDataProducer producer = None
    ):
        """
        Cached wrap for `BacktestDataProducer` class.

        producer : BacktestDataProducer
            BacktestDataProducer class.

        """
        self._producer = producer
        self.execution_resolutions = self._producer.execution_resolutions
        self.min_timestamp = self._producer.min_timestamp
        self.max_timestamp = self._producer.max_timestamp
        self.has_tick_data = False

        self._tick_cache = []
        self._ts_cache = []
        self._create_tick_cache()

    cdef void _create_tick_cache(self) except *:
        cdef Tick tick
        timing_start_total = datetime.utcnow()

        self._producer.setup(self.min_timestamp, self.max_timestamp)
        while self._producer.has_tick_data:
            tick = self._producer.next_tick()
            self._tick_cache.append(tick)
            self._ts_cache.append(tick.timestamp.timestamp())

        processing_time = round((datetime.utcnow() - timing_start_total).total_seconds(), 2)
        self._producer._log.info(f"Pre-caching {len(self._tick_cache):,} "
                       f"total tick rows in {processing_time}s.")
        self._clear_data()

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

        # Set indexing
        self._tick_index = bisect_left(self._ts_cache, start.timestamp())
        self._tick_index_last = bisect_left(self._ts_cache, stop.timestamp())
        self._init_start_tick_index = self._tick_index
        self._init_stop_tick_index = self._tick_index_last
        self.has_tick_data = True

    cpdef Tick next_tick(self):
        cdef Tick tick
        if self._tick_index <= self._tick_index_last:
            tick = self._tick_cache[self._tick_index]
            self._tick_index += 1

        if self._tick_index > self._tick_index_last:
            self.has_tick_data = False
        return tick

    cpdef void reset(self) except *:
        self._tick_index = self._init_start_tick_index
        self._tick_index_last = self._init_stop_tick_index
        self.has_tick_data = True

    cdef void _clear_data(self) except *:
        """
        Clear the data producer keep only pre-processed data.

        """
        self._producer.clear()

        gc.collect()  # Garbage collection to remove redundant processing artifacts
