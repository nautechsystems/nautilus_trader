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

# cython: boundscheck=False
# cython: wraparound=False

import gc

import numpy as np
import pandas as pd

from cpython.datetime cimport datetime

from pandas import DatetimeIndex

from nautilus_trader.backtest.clock cimport TestClock
from nautilus_trader.backtest.uuid cimport TestUUIDFactory
from nautilus_trader.common.data cimport DataClient
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.market cimport TickDataWrangler
from nautilus_trader.common.timer cimport TimeEventHandler
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport format_bytes
from nautilus_trader.core.functions cimport get_size_of
from nautilus_trader.core.functions cimport slice_dataframe
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation cimport bar_aggregation_to_string
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
        Initialize a new instance of the BacktestDataContainer class.
        """
        self.symbols = set()   # type: {Instrument}
        self.instruments = {}  # type: {Symbol, Instrument}
        self.ticks = {}        # type: {Symbol, pd.DataFrame}
        self.bars_bid = {}     # type: {Symbol, {BarAggregation, pd.DataFrame}}
        self.bars_ask = {}     # type: {Symbol, {BarAggregation, pd.DataFrame}}

    cpdef void add_instrument(self, Instrument instrument) except *:
        """
        Add the instrument to the container.

        :param instrument: The instrument to add.
        """
        Condition.not_none(instrument, "instrument")

        self.instruments[instrument.symbol] = instrument
        self.instruments = dict(sorted(self.instruments.items()))

    cpdef void add_quote_ticks(self, Symbol symbol, data: pd.DataFrame) except *:
        """
        Add the tick data to the container.

        :param symbol: The symbol for the tick data.
        :param data: The tick data to add.
        :raises TypeError: If data is a type other than DataFrame.
        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(data, "data")
        Condition.type(data, pd.DataFrame, "data")

        self.symbols.add(symbol)
        self.ticks[symbol] = data
        self.ticks = dict(sorted(self.ticks.items()))

    cpdef void add_bars(self, Symbol symbol, BarAggregation aggregation, PriceType price_type, data: pd.DataFrame) except *:
        """
        Add the bar data to the container.

        :param symbol: The symbol for the bar data.
        :param aggregation: The bar aggregation of the data.
        :param price_type: The price type of the data.
        :param data: The bar data to add.
        :raises TypeError: If data is a type other than DataFrame.
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

        :raises: AssertionFailed: If the any integrity check fails.
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

    def __init__(self,
                 BacktestDataContainer data not None,
                 int tick_capacity,
                 int bar_capacity,
                 TestClock clock not None,
                 Logger logger not None):
        """
        Initialize a new instance of the BacktestDataClient class.

        :param data: The data needed for the backtest data client.
        :param tick_capacity: The max length of the internal tick deques.
        :param bar_capacity: The max length of the internal bar deques.
        :param clock: The clock for the component.
        :param logger: The logger for the component.
        :raises ValueError: If the tick_capacity is not positive (> 0).
        :raises ValueError: If the bar_capacity is not positive (> 0).
        """
        Condition.positive_int(tick_capacity, "tick_capacity")
        Condition.positive_int(bar_capacity, "bar_capacity")
        super().__init__(
            tick_capacity=tick_capacity,
            bar_capacity=bar_capacity,
            use_previous_close=False,  # To correctly reproduce historical data bars
            clock=clock,
            uuid_factory=TestUUIDFactory(),
            logger=logger)

        # Check data integrity
        data.check_integrity()
        self._data = data

        cdef int counter = 0
        self._symbol_index = {}
        self._price_precisions = {}
        self._size_precisions = {}

        # Prepare instruments
        for instrument in self._data.instruments.values():
            self.handle_instrument(instrument)

        # Prepare data
        cdef list tick_frames = []
        self.execution_resolutions = []

        timing_start_total = datetime.utcnow()
        for symbol, instrument in self._instruments.items():
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
                data_bars_ask=None if symbol not in self._data.bars_ask else self._data.bars_ask[symbol])

            # Build data
            wrangler.build(counter)
            tick_frames.append(wrangler.tick_data)
            counter += 1

            self.execution_resolutions.append(f"{symbol.to_string()}={bar_aggregation_to_string(wrangler.resolution)}")
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

        :param start: The start datetime (UTC) for the run.
        :param stop: The stop datetime (UTC) for the run.
        """
        Condition.not_none(start, "start")
        Condition.not_none(stop, "stop")

        # Prepare instruments
        for instrument in self._data.instruments.values():
            self.handle_instrument(instrument)

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

        :return: Tick.
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
            self._timestamps[self._index])

        self._index += 1
        if self._index > self._index_last:
            self.has_data = False

        return tick

    cpdef void connect(self) except *:
        """
        Connect to the data service.
        """
        self._log.info("Connected.")

    cpdef void disconnect(self) except *:
        """
        Disconnect from the data service.
        """
        self._log.info("Disconnected.")

    cpdef void reset(self) except *:
        """
        Reset the client to its initial state.
        """
        self._log.debug(f"Resetting...")

        self._symbols = None
        self._price_volume = None
        self._timestamps = None
        self._index = 0
        self._index_last = len(self._tick_data) - 1
        self.has_data = False
        self._reset()

        self._log.info("Reset.")

    cpdef void dispose(self) except *:
        """
        Dispose of the data client by releasing all resources.
        """
        pass

    cpdef void process_tick(self, QuoteTick tick) except *:
        """
        Process the given tick with the data client.

        :param tick: The tick to process.
        """
        Condition.not_none(tick, "tick")

        self.handle_quote_tick(tick)

        if self._clock.timer_count == 0 or tick.timestamp < self._clock.next_event_time:
            return  # No events to handle yet

        cdef TimeEventHandler event_handler
        for event_handler in self._clock.advance_time(tick.timestamp):
            event_handler.handle()

    cpdef void request_quote_ticks(
            self,
            Symbol symbol,
            datetime from_datetime,
            datetime to_datetime,
            int limit,
            callback: callable) except *:
        """
        Request the historical quote ticks for the given parameters from the data service.

        :param symbol: The symbol for the ticks to download.
        :param from_datetime: The from datetime for the request.
        :param to_datetime: The to datetime for the request.
        :param limit: The limit for the number of ticks in the response (default = no limit) (>= 0).
        :param callback: The callback for the response.
        :raises ValueError: If limit is negative (< 0).
        :raises TypeError: If callback is not of type callable.
        """
        Condition.not_none(symbol, "symbol")
        Condition.not_negative_int(limit, "limit")
        Condition.callable(callback, "callback")

        self._log.info(f"Simulated request quote ticks for {symbol} from {from_datetime} to {to_datetime}.")

    cpdef void request_trade_ticks(
            self,
            Symbol symbol,
            datetime from_datetime,
            datetime to_datetime,
            int limit,
            callback: callable) except *:
        """
        Request the historical trade ticks for the given parameters from the data service.

        :param symbol: The symbol for the ticks to download.
        :param from_datetime: The from datetime for the request.
        :param to_datetime: The to datetime for the request.
        :param limit: The limit for the number of ticks in the response (default = no limit) (>= 0).
        :param callback: The callback for the response.
        :raises ValueError: If limit is negative (< 0).
        :raises TypeError: If callback is not of type callable.
        """
        Condition.not_none(symbol, "symbol")
        Condition.not_negative_int(limit, "limit")
        Condition.callable(callback, "callback")

        self._log.info(f"Simulated request trade ticks for {symbol} from {from_datetime} to {to_datetime}.")

    cpdef void request_bars(
            self,
            BarType bar_type,
            datetime from_datetime,
            datetime to_datetime,
            int limit,
            callback: callable) except *:
        """
        Request the historical bars for the given parameters from the data service.

        :param bar_type: The bar type for the bars to download.
        :param from_datetime: The from datetime for the request.
        :param to_datetime: The to datetime for the request.
        :param limit: The limit for the number of ticks in the response (default = no limit) (>= 0).
        :param callback: The callback for the response.
        :raises ValueError: If limit is negative (< 0).
        :raises TypeError: If callback is not of type callable.
        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_negative_int(limit, "limit")
        Condition.callable(callback, "callback")

        self._log.info(f"Simulated request bars for {bar_type} from {from_datetime} to {to_datetime}.")

    cpdef void request_instrument(self, Symbol symbol, callback: callable) except *:
        """
        Request the instrument for the given symbol.

        :param symbol: The symbol to update.
        :param callback: The callback for the response.
        :raises TypeError: If callback is not of type callable.
        """
        Condition.not_none(symbol, "symbol")
        Condition.callable(callback, "callback")

        self._log.info(f"Requesting instrument for {symbol}...")

        callback(self._instruments[symbol])

    cpdef void request_instruments(self, Venue venue, callback: callable) except *:
        """
        Request all instrument for given venue.

        :param venue: The venue for the request.
        :param callback: The callback for the response.
        :raises TypeError: If callback is not of type callable.
        """
        Condition.callable(callback, "callback")

        self._log.info(f"Requesting all instruments for the {venue} ...")

        callback(self.get_instruments())

    cpdef void subscribe_quote_ticks(self, Symbol symbol, handler: callable) except *:
        """
        Subscribe to quote tick data for the given symbol.

        :param symbol: The tick symbol to subscribe to.
        :param handler: The callable handler for subscription.
        :raises ValueError: If symbol is not a key in data_providers.
        :raises TypeError: If handler is not of type callable.
        """
        Condition.not_none(symbol, "symbol")
        Condition.callable(handler, "handler")

        self._add_quote_tick_handler(symbol, handler)

    cpdef void subscribe_trade_ticks(self, Symbol symbol, handler: callable) except *:
        """
        Subscribe to trade tick data for the given symbol.

        :param symbol: The tick symbol to subscribe to.
        :param handler: The callable handler for subscription.
        :raises ValueError: If symbol is not a key in data_providers.
        :raises TypeError: If handler is not of type callable.
        """
        Condition.not_none(symbol, "symbol")
        Condition.callable(handler, "handler")

        self._add_trade_tick_handler(symbol, handler)

    cpdef void subscribe_bars(self, BarType bar_type, handler: callable) except *:
        """
        Subscribe to live bar data for the given bar parameters.

        :param bar_type: The bar type to subscribe to.
        :param handler: The callable handler for subscription.
        :raises ValueError: If symbol is not a key in data_providers.
        :raises TypeError: If handler is not of type callable or None.
        """
        Condition.not_none(bar_type, "bar_type")
        Condition.callable_or_none(handler, "handler")

        self._start_generating_bars(bar_type, handler)

    cpdef void subscribe_instrument(self, Symbol symbol, handler: callable) except *:
        """
        Subscribe to live instrument data updates for the given symbol and handler.

        :param symbol: The instrument symbol to subscribe to.
        :param handler: The callable handler for subscription.
        :raises TypeError: If handler is not of type callable or None.
        """
        Condition.not_none(symbol, "symbol")
        Condition.callable_or_none(handler, "handler")

        if symbol not in self._instrument_handlers:
            self._log.info(f"Simulated subscribe to {symbol} instrument updates "
                           f"(a backtest data client wont update an instrument).")

    cpdef void unsubscribe_quote_ticks(self, Symbol symbol, handler: callable) except *:
        """
        Unsubscribes from quote tick data for the given symbol.

        :param symbol: The tick symbol to unsubscribe from.
        :param handler: The callable handler which was subscribed.
        :raises ValueError: If symbol is not a key in data_providers.
        :raises TypeError: If handler is not of type callable or None.
        """
        Condition.not_none(symbol, "symbol")
        Condition.callable_or_none(handler, "handler")

        self._remove_quote_tick_handler(symbol, handler)

    cpdef void unsubscribe_trade_ticks(self, Symbol symbol, handler: callable) except *:
        """
        Unsubscribes from trade tick data for the given symbol.

        :param symbol: The tick symbol to unsubscribe from.
        :param handler: The callable handler which was subscribed.
        :raises ValueError: If symbol is not a key in data_providers.
        :raises TypeError: If handler is not of type callable or None.
        """
        Condition.not_none(symbol, "symbol")
        Condition.callable_or_none(handler, "handler")

        self._remove_trade_tick_handler(symbol, handler)

    cpdef void unsubscribe_bars(self, BarType bar_type, handler: callable) except *:
        """
        Unsubscribes from bar data for the given symbol and venue.

        :param bar_type: The bar type to unsubscribe from.
        :param handler: The callable handler which was subscribed.
        :raises ValueError: If symbol is not a key in data_providers.
        :raises TypeError: If handler is not of type callable or None.
        """
        Condition.not_none(bar_type, "bar_type")
        Condition.callable_or_none(handler, "handler")

        self._stop_generating_bars(bar_type, handler)

    cpdef void unsubscribe_instrument(self, Symbol symbol, handler: callable) except *:
        """
        Unsubscribe from live instrument data updates for the given symbol and handler.

        :param symbol: The instrument symbol to unsubscribe from.
        :param handler: The callable handler which was subscribed.
        :raises TypeError: If handler is not of type Callable.
        """
        Condition.not_none(symbol, "symbol")
        Condition.callable_or_none(handler, "handler")

        self._log.info(f"Simulated unsubscribe from {symbol} instrument updates "
                       f"(a backtest data client will not update an instrument).")

    cpdef void update_instruments(self, Venue venue) except *:
        """
        Update all instruments from the database.
        """
        self._log.info(f"Simulated update all instruments for the {venue} venue "
                       f"(a backtest data client already has all instruments needed).")
