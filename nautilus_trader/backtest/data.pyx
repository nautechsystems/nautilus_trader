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

A `BacktestDataContainer` is
a convenient container for holding and organizing backtest related data - which can be passed
to one or more `BacktestDataEngine`(s).
"""

import gc

import numpy as np
import pandas as pd

from cpython.datetime cimport datetime

from pandas import DatetimeIndex

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport format_bytes
from nautilus_trader.core.functions cimport get_size_of
from nautilus_trader.core.functions cimport slice_dataframe
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.data.wrangling cimport TickDataWrangler
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregationParser
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
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
        self.ticks = {}        # type: {Symbol, pd.DataFrame}
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
        Add the tick data to the container.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the tick data.
        data : pd.DataFrame
            The tick data to add.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(data, "data")
        Condition.type(data, pd.DataFrame, "data")

        self.symbols.add(symbol)
        self.ticks[symbol] = data
        self.ticks = dict(sorted(self.ticks.items()))

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
        AssertionFailed
            If the any integrity check fails.

        """
        # Check there is the needed instrument for each data symbol
        for symbol in self.symbols:
            assert(symbol in self.instruments, f"The needed instrument {symbol} was not provided.")

        # Check that all bar DataFrames for each symbol are of the same shape and index
        cdef dict shapes = {}  # type: {BarAggregation, tuple}
        cdef dict indexs = {}  # type: {BarAggregation, DatetimeIndex}
        for symbol, data in self.bars_bid.items():
            for aggregation, dataframe in data.items():
                if aggregation not in shapes:
                    shapes[aggregation] = dataframe.shape
                if aggregation not in indexs:
                    indexs[aggregation] = dataframe.index
                assert(dataframe.shape == shapes[aggregation], f"{dataframe} shape is not equal.")
                assert(dataframe.index == indexs[aggregation], f"{dataframe} index is not equal.")
        for symbol, data in self.bars_ask.items():
            for aggregation, dataframe in data.items():
                assert(dataframe.shape == shapes[aggregation], f"{dataframe} shape is not equal.")
                assert(dataframe.index == indexs[aggregation], f"{dataframe} index is not equal.")

    cpdef long total_data_size(self):
        cdef long size = 0
        size += get_size_of(self.ticks)
        size += get_size_of(self.bars_bid)
        size += get_size_of(self.bars_ask)
        return size


cdef class BacktestDataClient(DataClient):
    """
    Provides a data client for backtesting.
    """

    def __init__(
            self,
            BacktestDataContainer data not None,
            Venue venue not None,
            DataEngine engine not None,
            Clock clock not None,
            UUIDFactory uuid_factory not None,
            Logger logger not None,
    ):
        """
        Initialize a new instance of the `BacktestDataClient` class.

        venue : Venue
            The venue the client can provide data for.
        engine : DataEngine
            The data engine to connect to the client.
        clock : Clock
            The clock for the component.
        uuid_factory : UUIDFactory
            The UUID factory for the component.
        logger : Logger
            The logger for the component.

        """
        super().__init__(
            venue,
            engine,
            clock,
            uuid_factory,
            logger,
        )

        # Check data integrity
        data.check_integrity()
        self._data = data

        cdef int counter = 0
        self._symbol_index = {}
        self._price_precisions = {}
        self._size_precisions = {}

        # Prepare instruments
        for instrument in self._data.instruments.values():
            self._engine.process(instrument)

        # Prepare data
        cdef list tick_frames = []
        self.execution_resolutions = []

        timing_start_total = datetime.utcnow()
        for instrument in data.instruments.values():
            symbol = instrument.symbol
            self._log.info(f"Preparing {symbol} data...")
            timing_start = datetime.utcnow()

            self._symbol_index[counter] = symbol
            self._price_precisions[counter] = instrument.price_precision
            self._size_precisions[counter] = instrument.size_precision

            # Build data wrangler
            wrangler = TickDataWrangler(
                instrument=instrument,
                data_ticks=None if symbol not in self._data.ticks else self._data.ticks[symbol],
                data_bars_bid=None if symbol not in self._data.bars_bid else self._data.bars_bid[symbol],
                data_bars_ask=None if symbol not in self._data.bars_ask else self._data.bars_ask[symbol],
            )

            # Build data
            wrangler.pre_process(counter)
            tick_frames.append(wrangler.tick_data)
            counter += 1

            self.execution_resolutions.append(f"{symbol}={BarAggregationParser.to_string(wrangler.resolution)}")
            self._log.info(f"Prepared {len(wrangler.tick_data):,} {symbol} ticks in "
                           f"{round((datetime.utcnow() - timing_start).total_seconds(), 2)}s.")

            # Dump data artifacts
            del wrangler

        # Merge and sort all ticks
        self._log.info(f"Merging tick data stream...")
        self._tick_data = pd.concat(tick_frames)
        self._tick_data.sort_index(axis=0, kind="mergesort", inplace=True)

        # Set min and max timestamps
        self.min_timestamp = self._tick_data.index.min()
        self.max_timestamp = self._tick_data.index.max()

        self._symbols = None
        self._price_volume = None
        self._timestamps = None
        self._index = 0
        self._index_last = len(self._tick_data) - 1
        self.has_data = False

        self._log.info(f"Prepared {len(self._tick_data):,} ticks total in "
                       f"{round((datetime.utcnow() - timing_start_total).total_seconds(), 2)}s.")

        gc.collect()  # Garbage collection

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

        # Build tick data stream
        data_slice = slice_dataframe(self._tick_data, start, stop)  # See function comments on why [:] isn't used
        self._symbols = data_slice["symbol"].to_numpy(dtype=np.ushort)
        self._price_volume = data_slice[["bid", "ask", "bid_size", "ask_size"]].to_numpy(dtype=np.double)
        self._timestamps = np.asarray([<datetime>dt for dt in data_slice.index])

        self._index = 0
        self._index_last = len(data_slice) - 1
        self.has_data = True

        cdef long total_size = 0
        total_size += get_size_of(self._symbols)
        total_size += get_size_of(self._price_volume)
        total_size += get_size_of(self._timestamps)
        self._log.info(f"Data stream size: {format_bytes(total_size)}")

    cdef QuoteTick generate_tick(self):
        """
        Generate the next tick in the ordered data sequence.

        Returns
        -------
        QuoteTick

        """
        cdef int symbol_indexer = self._symbols[self._index]
        cdef int price_precision = self._price_precisions[symbol_indexer]
        cdef int size_precision = self._size_precisions[symbol_indexer]
        cdef double[:] values = self._price_volume[self._index]

        cdef QuoteTick tick = QuoteTick(
            self._symbol_index[symbol_indexer],
            Price(values[0], price_precision),
            Price(values[1], price_precision),
            Quantity(values[2], size_precision),
            Quantity(values[3], size_precision),
            self._timestamps[self._index],
        )

        self._index += 1
        if self._index > self._index_last:
            self.has_data = False

        return tick

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void connect(self) except *:
        pass  # NO-OP for backtest engine

    cpdef void disconnect(self) except *:
        pass  # NO-OP for backtest engine

    cpdef void reset(self) except *:
        """
        Reset the data client.

        All stateful values are reset to their initial value.
        """
        self._log.debug(f"Resetting...")

        self._symbols = None
        self._price_volume = None
        self._timestamps = None
        self._index = 0
        self._index_last = len(self._tick_data) - 1
        self.has_data = False

        self._log.info("Reset.")

    cpdef void dispose(self) except *:
        """
        Dispose of the data client.
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
        # Do nothing for backtest

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
        # Do nothing for backtest

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
        # Do nothing for backtest

    cpdef void request_instrument(self, Symbol symbol, UUID correlation_id) except *:
        Condition.not_none(symbol, "symbol")
        Condition.not_none(correlation_id, "correlation_id")

        cdef Instrument instrument = self._data.instruments.get(symbol)

        if instrument is None:
            self._log.warning(f"No instrument found for {symbol}.")
            return

        self._handle_instruments([instrument], correlation_id)

    cpdef void request_instruments(self, UUID correlation_id) except *:
        self._handle_instruments(list(self._data.instruments.values()), correlation_id)

# -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    cpdef void subscribe_quote_ticks(self, Symbol symbol) except *:
        pass
        # Do nothing for backtest

    cpdef void subscribe_trade_ticks(self, Symbol symbol) except *:
        pass
        # Do nothing for backtest

    cpdef void subscribe_bars(self, BarType bar_type) except *:
        self._log.error(f"Cannot subscribe to externally aggregated bars "
                        f"(backtesting only supports internal aggregation at this stage).")

    cpdef void subscribe_instrument(self, Symbol symbol) except *:
        pass
        # Do nothing for backtest

    cpdef void unsubscribe_quote_ticks(self, Symbol symbol) except *:
        pass
        # Do nothing for backtest

    cpdef void unsubscribe_trade_ticks(self, Symbol symbol) except *:
        pass
        # Do nothing for backtest

    cpdef void unsubscribe_bars(self, BarType bar_type) except *:
        self._log.error(f"Cannot unsubscribe from externally aggregated bars "
                        f"(backtesting only supports internal aggregation at this stage).")

    cpdef void unsubscribe_instrument(self, Symbol symbol) except *:
        pass
        # Do nothing for backtest
