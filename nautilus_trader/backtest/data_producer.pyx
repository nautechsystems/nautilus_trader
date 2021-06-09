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

from bisect import bisect_left
import gc

import numpy as np
import pandas as pd

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta
from libc.stdint cimport uint64_t

from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.datetime cimport as_utc_timestamp
from nautilus_trader.core.datetime cimport dt_to_unix_nanos
from nautilus_trader.core.datetime cimport nanos_to_unix_dt
from nautilus_trader.core.functions cimport slice_dataframe
from nautilus_trader.core.time cimport unix_timestamp
from nautilus_trader.data.wrangling cimport QuoteTickDataWrangler
from nautilus_trader.data.wrangling cimport TradeTickDataWrangler
from nautilus_trader.model.c_enums.aggressor_side cimport AggressorSideParser
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregationParser
from nautilus_trader.model.data cimport Data
from nautilus_trader.model.identifiers cimport TradeMatchId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.tick cimport QuoteTick


cdef class DataProducerFacade:
    """
    Provides a read-only facade for data producers.
    """

    cpdef list instruments(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void reset(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Data next(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")


cdef class BacktestDataProducer(DataProducerFacade):
    """
    Provides a basic data producer for backtesting.
    """

    def __init__(
        self,
        Logger logger not None,
        list instruments=None,
        list generic_data=None,
        list order_book_data=None,
        dict quote_ticks=None,
        dict trade_ticks=None,
        dict bars_bid=None,
        dict bars_ask=None,
    ):
        """
        Initialize a new instance of the ``BacktestDataProducer`` class.

        Parameters
        ----------
        instruments : list[Instrument]
            The instruments for backtesting.
        generic_data : list[GenericData]
            The generic data for backtesting.
        order_book_data : list[OrderBookData]
            The order book data for backtesting.
        quote_ticks : dict[InstrumentId, pd.DataFrame]
            The quote tick data for backtesting.
        trade_ticks : dict[InstrumentId, pd.DataFrame]
            The trade tick data for backtesting.
        bars_bid : dict[InstrumentId, dict[BarAggregation, pd.DataFrame]]
            The bid bar data for backtesting.
        bars_ask : dict[InstrumentId, dict[BarAggregation, pd.DataFrame]]
            The ask bar data for backtesting.
        logger : Logger
            The logger for the component.

        """
        if instruments is None:
            instruments = []
        if generic_data is None:
            generic_data = []
        if order_book_data is None:
            order_book_data = []
        if quote_ticks is None:
            quote_ticks = {}
        if trade_ticks is None:
            trade_ticks = {}
        if bars_bid is None:
            bars_bid = {}
        if bars_ask is None:
            bars_ask = {}

        self._log = LoggerAdapter(
            component=type(self).__name__,
            logger=logger,
        )

        # Save instruments
        self._instruments = instruments
        cdef int instrument_counter = 0
        self._instrument_index = {}

        # Merge data stream
        self._stream = sorted(
            generic_data + order_book_data,
            key=lambda x: x.ts_recv_ns,
        )

        # Check bar data integrity
        for instrument in self._instruments:
            # Check symmetry of bid ask bar data
            if bars_bid is None:
                bid_bars_keys = None
            else:
                bid_bars_keys = bars_bid.get(instrument.id, {}).keys()

            if bars_ask is None:
                ask_bars_keys = None
            else:
                ask_bars_keys = bars_ask.get(instrument.id, {}).keys()

            if bid_bars_keys != ask_bars_keys:
                raise RuntimeError(f"Bar data mismatch for {instrument.id}")

        # Prepare tick data
        self._quote_tick_data = pd.DataFrame()
        self._trade_tick_data = pd.DataFrame()
        cdef list quote_tick_frames = []
        cdef list trade_tick_frames = []
        self.execution_resolutions = []

        cdef double ts_total = unix_timestamp()

        for instrument in self._instruments:
            instrument_id = instrument.id
            self._log.info(f"Preparing {instrument_id} data...")

            self._instrument_index[instrument_counter] = instrument_id

            execution_resolution = None

            # Process quote tick data
            # -----------------------
            if instrument_id in quote_ticks or instrument_id in bars_bid:
                ts = unix_timestamp()  # Time data processing
                quote_wrangler = QuoteTickDataWrangler(
                    instrument=instrument,
                    data_quotes=quote_ticks.get(instrument_id),
                    data_bars_bid=bars_bid.get(instrument_id),
                    data_bars_ask=bars_ask.get(instrument_id),
                )

                # noinspection PyUnresolvedReferences
                quote_wrangler.pre_process(instrument_counter)
                quote_tick_frames.append(quote_wrangler.processed_data)

                execution_resolution = BarAggregationParser.to_str(quote_wrangler.resolution)
                self._log.info(f"Prepared {len(quote_wrangler.processed_data):,} {instrument_id} quote tick rows in "
                               f"{unix_timestamp() - ts:.3f}s.")
                del quote_wrangler  # Dump processing artifact

            # Process trade tick data
            # -----------------------
            if instrument_id in trade_ticks:
                if isinstance(trade_ticks[instrument_id], pd.DataFrame):
                    ts = unix_timestamp()  # Time data processing
                    trade_wrangler = TradeTickDataWrangler(
                        instrument=instrument,
                        data=trade_ticks.get(instrument_id),
                    )

                    # noinspection PyUnresolvedReferences
                    trade_wrangler.pre_process(instrument_counter)
                    trade_tick_frames.append(trade_wrangler.processed_data)

                    execution_resolution = BarAggregationParser.to_str(BarAggregation.TICK)
                    self._log.info(f"Prepared {len(trade_wrangler.processed_data):,} {instrument_id} trade tick rows in "
                                   f"{unix_timestamp() - ts:.3f}s.")
                    del trade_wrangler  # Dump processing artifact
                elif isinstance(trade_ticks[instrument_id], list):
                    # We have a list of TradeTick objects
                    self._stream = sorted(
                        self._stream + trade_ticks[instrument_id], key=lambda x: x.ts_recv_ns,
                    )

            # TODO: Execution resolution
            # if instrument_id in data.books:
            #     execution_resolution = "ORDER_BOOK"
            #
            # if execution_resolution is None:
            #     raise RuntimeError(f"No execution level data for {instrument_id}")

            # Increment counter for indexing the next instrument
            instrument_counter += 1

            self.execution_resolutions.append(f"{instrument_id}={execution_resolution}")

        # Merge and sort all ticks
        if quote_tick_frames:
            self._log.info(f"Merging QuoteTick data streams...")
            self._quote_tick_data = pd.concat(quote_tick_frames)
            self._quote_tick_data.sort_index(axis=0, kind="mergesort", inplace=True)

        if trade_tick_frames:
            self._log.info(f"Merging TradeTick data streams...")
            self._trade_tick_data = pd.concat(trade_tick_frames)
            self._trade_tick_data.sort_index(axis=0, kind="mergesort", inplace=True)

        # TODO: Refactor timestamping below
        # Set timestamps
        cdef datetime min_timestamp = None
        cdef datetime max_timestamp = None

        if not self._quote_tick_data.empty:
            min_timestamp = self._quote_tick_data.index.min()
            max_timestamp = self._quote_tick_data.index.max()

        if not self._trade_tick_data.empty:
            if min_timestamp is None:
                min_timestamp = self._trade_tick_data.index.min()
            else:
                min_timestamp = min(self._quote_tick_data.index.min(), self._trade_tick_data.index.min())

            if max_timestamp is None:
                max_timestamp = self._trade_tick_data.index.max()
            else:
                max_timestamp = max(self._quote_tick_data.index.max(), self._trade_tick_data.index.max())

        if min_timestamp is None:
            min_timestamp = as_utc_timestamp(pd.Timestamp.max)

        if max_timestamp is None:
            max_timestamp = as_utc_timestamp(pd.Timestamp.min)

        self.min_timestamp_ns = dt_to_unix_nanos(min_timestamp)
        self.max_timestamp_ns = dt_to_unix_nanos(max_timestamp)

        if self._stream:
            self.min_timestamp_ns = min(self.min_timestamp_ns, self._stream[0].ts_recv_ns)
            self.max_timestamp_ns = max(self.max_timestamp_ns, self._stream[-1].ts_recv_ns)

        self.min_timestamp = as_utc_timestamp(nanos_to_unix_dt(self.min_timestamp_ns))
        self.max_timestamp = as_utc_timestamp(nanos_to_unix_dt(self.max_timestamp_ns))

        # Initialize backing fields
        self._stream_index = 0
        self._stream_index_last = 0
        self._next_data = None

        self._quote_instruments = None
        self._quote_bids = None
        self._quote_asks = None
        self._quote_bid_sizes = None
        self._quote_ask_sizes = None
        self._quote_timestamps = None
        self._quote_index = 0
        self._quote_index_last = 0
        self._next_quote_tick = None

        self._trade_instruments = None
        self._trade_prices = None
        self._trade_sizes = None
        self._trade_match_ids = None
        self._trade_sides = None
        self._trade_timestamps = None
        self._trade_index = 0
        self._trade_index_last = 0
        self._next_trade_tick = None

        self.has_data = False

        total_elements = len(self._quote_tick_data) + len(self._trade_tick_data) + len(self._stream)

        self._log.info(f"Prepared {total_elements:,} total data elements "
                       f"in {unix_timestamp() - ts_total:.3f}s.")

        gc.collect()  # Garbage collection to remove redundant processing artifacts

    cpdef LoggerAdapter get_logger(self):
        """
        Return the logger for the component.

        Returns
        -------
        LoggerAdapter

        """
        return self._log

    cpdef list instruments(self):
        """
        Return the instruments held by the data producer.

        Returns
        -------
        list[Instrument]

        """
        return self._instruments.copy()

    def setup(self, int64_t start_ns, int64_t stop_ns):
        """
        Setup tick data for a backtest run.

        Parameters
        ----------
        start_ns : int64
            The UNIX timestamp (nanos) for the run start.
        stop_ns : int64
            The UNIX timestamp (nanos) for the run stop.

        """
        self._log.info(f"Pre-processing data stream...")

        # Calculate data size
        cdef uint64_t total_size = 0

        if self._stream:
            # Set data stream start index
            self._stream_index = next(
                idx for idx, data in enumerate(self._stream) if start_ns <= data.ts_recv_ns
            )

            # Set data stream stop index
            self._stream_index_last = len(self._stream) - 1 - next(
                idx for idx, data in enumerate(reversed(self._stream)) if stop_ns <= data.ts_recv_ns
            )

            # Prepare initial data
            self._iterate_stream()

        cdef datetime start = nanos_to_unix_dt(start_ns)
        cdef datetime stop = nanos_to_unix_dt(stop_ns)

        # Build quote tick data stream
        if not self._quote_tick_data.empty:
            time_buffer = timedelta(milliseconds=1)  # To ensure we don't pickup an `unwanted` generated tick
            # See slice_dataframe function comments on why [:] isn't used
            quote_ticks_slice = slice_dataframe(self._quote_tick_data, start + time_buffer, stop)

            self._quote_instruments = quote_ticks_slice["instrument_id"].to_numpy(dtype=np.ushort)
            self._quote_bids = quote_ticks_slice["bid"].values
            self._quote_asks = quote_ticks_slice["ask"].values
            self._quote_bid_sizes = quote_ticks_slice["bid_size"].values
            self._quote_ask_sizes = quote_ticks_slice["ask_size"].values
            self._quote_timestamps = np.asarray(
                [dt_to_unix_nanos(dt) for dt in quote_ticks_slice.index],
                dtype=np.int64,
            )

            # Set indexing
            self._quote_index = 0
            self._quote_index_last = len(quote_ticks_slice) - 1

            # Prepare initial tick
            self._iterate_quote_ticks()

        # Build trade tick data stream
        if not self._trade_tick_data.empty:
            # See slice_dataframe function comments on why [:] isn't used
            trade_ticks_slice = slice_dataframe(self._trade_tick_data, start, stop)

            self._trade_instruments = trade_ticks_slice["instrument_id"].to_numpy(dtype=np.ushort)
            self._trade_prices = trade_ticks_slice["price"].values
            self._trade_sizes = trade_ticks_slice["quantity"].values
            self._trade_match_ids = trade_ticks_slice["match_id"].values
            self._trade_sides = trade_ticks_slice["aggressor_side"].values
            self._trade_timestamps = np.asarray(
                [dt_to_unix_nanos(dt) for dt in trade_ticks_slice.index],
                dtype=np.int64,
            )

            # Set indexing
            self._trade_index = 0
            self._trade_index_last = len(trade_ticks_slice) - 1

            # Prepare initial tick
            self._iterate_trade_ticks()

        self.has_data = True

    cpdef void reset(self) except *:
        """
        Reset the data producer.

        All stateful fields are reset to their initial value.
        """
        self._log.info(f"Resetting...")

        self._stream_index = 0
        self._stream_index_last = len(self._stream) - 1
        self._next_data = None

        # Clear pre-processed quote tick data
        self._quote_instruments = None
        self._quote_bids = None
        self._quote_asks = None
        self._quote_bid_sizes = None
        self._quote_ask_sizes = None
        self._quote_timestamps = None
        self._quote_index = 0
        self._quote_index_last = len(self._quote_tick_data) - 1
        self._next_quote_tick = None

        # Clear pre-processed trade tick data
        self._trade_instruments = None
        self._trade_prices = None
        self._trade_sizes = None
        self._trade_match_ids = None
        self._trade_sides = None
        self._trade_timestamps = None
        self._trade_index = 0
        self._trade_index_last = len(self._quote_tick_data) - 1
        self._next_trade_tick = None

        self.has_data = False

        self._log.info("Reset.")

    cpdef void clear(self) except *:
        """
        Clears the original data from the producer.

        """
        self._trade_tick_data = pd.DataFrame()
        self._quote_tick_data = pd.DataFrame()
        gc.collect()  # Removes redundant processing artifacts

        self._log.info("Cleared.")

    cpdef Data next(self):
        """
        Return the next data item in the stream (if one exists).

        Checking `has_data` is `True` will ensure there is data.

        Returns
        -------
        Data or None

        """
        # Determine next data element
        cdef int64_t next_timestamp_ns = 9223372036854775807  # int64 max
        cdef int choice = 0

        if self._next_quote_tick is not None:
            next_timestamp_ns = self._next_quote_tick.ts_recv_ns
            choice = 1

        if self._next_trade_tick is not None:
            if choice == 0 or self._next_trade_tick.ts_recv_ns <= next_timestamp_ns:
                choice = 2

        cdef Data next_data = None
        if self._next_data is not None:
            if choice == 0 or self._next_data.ts_recv_ns <= next_timestamp_ns:
                next_data = self._next_data
                self._iterate_stream()
                return next_data

        if choice == 1:
            next_data = self._next_quote_tick
            self._iterate_quote_ticks()
        elif choice == 2:
            next_data = self._next_trade_tick
            self._iterate_trade_ticks()

        return next_data

    cdef void _iterate_stream(self) except *:
        if self._stream_index <= self._stream_index_last:
            self._next_data = self._stream[self._stream_index]
            self._stream_index += 1
        else:
            self._next_data = None
            if self._next_quote_tick is None and self._next_trade_tick is None:
                self.has_data = False

    cdef void _iterate_quote_ticks(self) except *:
        if self._quote_index <= self._quote_index_last:
            self._next_quote_tick = self._generate_quote_tick(self._quote_index)
            self._quote_index += 1
        else:
            self._next_quote_tick = None
            if self._next_data is None and self._next_trade_tick is None:
                self.has_data = False

    cdef void _iterate_trade_ticks(self) except *:
        if self._trade_index <= self._trade_index_last:
            self._next_trade_tick = self._generate_trade_tick(self._trade_index)
            self._trade_index += 1
        else:
            self._next_trade_tick = None
            if self._next_data is None and self._next_quote_tick is None:
                self.has_data = False

    cdef QuoteTick _generate_quote_tick(self, int index):
        return QuoteTick(
            instrument_id=self._instrument_index[self._quote_instruments[index]],
            bid=Price.from_str_c(self._quote_bids[index]),
            ask=Price.from_str_c(self._quote_asks[index]),
            bid_size=Quantity.from_str_c(self._quote_bid_sizes[index]),
            ask_size=Quantity.from_str_c(self._quote_ask_sizes[index]),
            ts_event_ns=self._quote_timestamps[index],
            ts_recv_ns=self._quote_timestamps[index],
        )

    cdef TradeTick _generate_trade_tick(self, int index):
        return TradeTick(
            instrument_id=self._instrument_index[self._trade_instruments[index]],
            price=Price.from_str_c(self._trade_prices[index]),
            size=Quantity.from_str_c(self._trade_sizes[index]),
            aggressor_side=AggressorSideParser.from_str(self._trade_sides[index]),
            match_id=TradeMatchId(self._trade_match_ids[index]),
            ts_event_ns=self._trade_timestamps[index],  # TODO(cs): Hardcoded identical for now
            ts_recv_ns=self._trade_timestamps[index],
        )


cdef class CachedProducer(DataProducerFacade):
    """
    Cached wrap for the `BacktestDataProducer`` class.
    """

    def __init__(self, BacktestDataProducer producer):
        """
        Initialize a new instance of the ``CachedProducer`` class.

        Parameters
        ----------
        producer : BacktestDataProducer
            The data producer to cache.

        """
        self._producer = producer
        self._log = producer.get_logger()
        self._timestamp_cache = []
        self._data_cache = []
        self._data_index = 0
        self._data_index_last = 0
        self._init_start_data_index = 0
        self._init_stop_data_index = 0

        self.execution_resolutions = self._producer.execution_resolutions
        self.min_timestamp = self._producer.min_timestamp
        self.max_timestamp = self._producer.max_timestamp
        self.min_timestamp_ns = self._producer.min_timestamp_ns
        self.max_timestamp_ns = self._producer.max_timestamp_ns
        self.has_data = False

        self._create_data_cache()

    cpdef list instruments(self):
        """
        Return the instruments held by the data producer.

        Returns
        -------
        list[Instrument]

        """
        return self._producer.instruments()

    def setup(self, int64_t start_ns, int64_t stop_ns):
        """
        Setup tick data for a backtest run.

        Parameters
        ----------
        start_ns : int64
            The UNIX timestamp (nanos) for the run start.
        stop_ns : int64
            The UNIX timestamp (nanos) for the run stop.

        """
        self._producer.setup(start_ns, stop_ns)

        # Set indexing
        self._data_index = bisect_left(self._timestamp_cache, start_ns)
        self._data_index_last = bisect_left(self._timestamp_cache, stop_ns)
        self._init_start_data_index = self._data_index
        self._init_stop_data_index = self._data_index_last
        self.has_data = True

    cpdef void reset(self) except *:
        """
        Reset the producer which sets the internal indexes to their initial

        All stateful fields are reset to their initial value.
        """
        self._data_index = self._init_start_data_index
        self._data_index_last = self._init_stop_data_index
        self.has_data = True

    cpdef Data next(self):
        """
        Return the next data item in the stream (if one exists).

        Checking `has_data` is `True` will ensure there is data.

        Returns
        -------
        Data or None

        """
        # Cython does not produce efficient generator code, and so we will
        # manually track the index for efficiency.
        cdef Data data = None
        if self.has_data:
            data = self._data_cache[self._data_index]
            self._data_index += 1

        # Check if last data item
        if self._data_index > self._data_index_last:
            self.has_data = False

        return data

    cdef void _create_data_cache(self) except *:
        self._log.info(f"Pre-caching data...")
        self._producer.setup(self.min_timestamp_ns, self.max_timestamp_ns)

        cdef double ts = unix_timestamp()

        cdef Data data
        while self._producer.has_data:
            data = self._producer.next()
            self._data_cache.append(data)
            self._timestamp_cache.append(data.ts_recv_ns)

        self._log.info(f"Pre-cached {len(self._data_cache):,} "
                       f"total data items in {unix_timestamp() - ts:.3f}s.")

        self._producer.reset()
        self._producer.clear()
        gc.collect()  # Removes redundant processing artifacts
