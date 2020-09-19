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

import pandas as pd

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.handlers cimport BarHandler
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.timer cimport Timer
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport as_utc_index
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarSpecification
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation cimport bar_aggregation_to_string
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.price_type cimport price_type_to_string
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class TickDataWrangler:
    """
    Provides a means of building lists of ticks from the given Pandas DataFrames
    of bid and ask data. Provided data can either be tick data or bar data.
    """

    def __init__(self,
                 Instrument instrument not None,
                 data_ticks: pd.DataFrame=None,
                 dict data_bars_bid=None,
                 dict data_bars_ask=None):
        """
        Initialize a new instance of the TickDataWrangler class.

        :param instrument: The instrument for the data wrangler.
        :param data_ticks: The optional pd.DataFrame containing the tick data.
        :param data_bars_bid: The optional dictionary containing the bars bid data.
        :param data_bars_ask: The optional dictionary containing the bars ask data.
        :raises: ValueError: If the tick_data is a type other than None or DataFrame.
        :raises: ValueError: If the bid_data is a type other than None or Dict.
        :raises: ValueError: If the ask_data is a type other than None or Dict.
        :raises: ValueError: If the tick_data is None and the bars data is None.
        """
        Condition.type_or_none(data_ticks, pd.DataFrame, "tick_data")
        Condition.type_or_none(data_bars_bid, dict, "bid_data")
        Condition.type_or_none(data_bars_ask, dict, "ask_data")

        if data_ticks is not None and len(data_ticks) > 0:
            self._data_ticks = as_utc_index(data_ticks)
        else:
            Condition.true(data_bars_bid is not None, "data_bars_bid is not None")
            Condition.true(data_bars_ask is not None, "data_bars_ask is not None")
            self._data_bars_bid = data_bars_bid
            self._data_bars_ask = data_bars_ask

        self.instrument = instrument

        self.tick_data = []
        self.resolution = BarAggregation.UNDEFINED

    cpdef void build(self, int symbol_indexer) except *:
        """
        Return the built ticks from the held data.

        :return List[Tick].
        """
        if self._data_ticks is not None and len(self._data_ticks) > 0:
            # Build ticks from data
            self.tick_data = self._data_ticks
            self.tick_data["symbol"] = symbol_indexer

            if "bid_size" not in self.tick_data.columns:
                self.tick_data["bid_size"] = 1.0

            if "ask_size" not in self.tick_data.columns:
                self.tick_data["ask_size"] = 1.0

            self.resolution = BarAggregation.TICK
            return

        # Build ticks from highest resolution bar data
        if BarAggregation.SECOND in self._data_bars_bid:
            bars_bid = self._data_bars_bid[BarAggregation.SECOND]
            bars_ask = self._data_bars_ask[BarAggregation.SECOND]
            self.resolution = BarAggregation.SECOND
        elif BarAggregation.MINUTE in self._data_bars_bid:
            bars_bid = self._data_bars_bid[BarAggregation.MINUTE]
            bars_ask = self._data_bars_ask[BarAggregation.MINUTE]
            self.resolution = BarAggregation.MINUTE
        elif BarAggregation.HOUR in self._data_bars_bid:
            bars_bid = self._data_bars_bid[BarAggregation.HOUR]
            bars_ask = self._data_bars_ask[BarAggregation.HOUR]
            self.resolution = BarAggregation.HOUR
        elif BarAggregation.DAY in self._data_bars_bid:
            bars_bid = self._data_bars_bid[BarAggregation.DAY]
            bars_ask = self._data_bars_ask[BarAggregation.DAY]
            self.resolution = BarAggregation.DAY

        Condition.not_none(bars_bid, "bars_bid")
        Condition.not_none(bars_ask, "bars_ask")
        Condition.true(len(bars_bid) > 0, "len(bars_bid) > 0")
        Condition.true(len(bars_ask) > 0, "len(bars_ask) > 0")
        Condition.true(all(bars_bid.index) == all(bars_ask.index), "bars_bid.index == bars_ask.index")
        Condition.true(bars_bid.shape == bars_ask.shape, "bars_bid.shape == bars_ask.shape")

        bars_bid = as_utc_index(bars_bid)
        bars_ask = as_utc_index(bars_ask)

        cdef dict data_open = {
            "bid": bars_bid["open"].values,
            "ask": bars_ask["open"].values,
            "bid_size": bars_bid["volume"].values,
            "ask_size": bars_ask["volume"].values
        }

        cdef dict data_high = {
            "bid": bars_bid["high"].values,
            "ask": bars_ask["high"].values,
            "bid_size": bars_bid["volume"].values,
            "ask_size": bars_ask["volume"].values
        }

        cdef dict data_low = {
            "bid": bars_bid["low"].values,
            "ask": bars_ask["low"].values,
            "bid_size": bars_bid["volume"].values,
            "ask_size": bars_ask["volume"].values
        }

        cdef dict data_close = {
            "bid": bars_bid["close"],
            "ask": bars_ask["close"],
            "bid_size": bars_bid["volume"],
            "ask_size": bars_ask["volume"]
        }

        df_ticks_o = pd.DataFrame(data=data_open, index=bars_bid.index.shift(periods=-100, freq="ms"))
        df_ticks_h = pd.DataFrame(data=data_high, index=bars_bid.index.shift(periods=-100, freq="ms"))
        df_ticks_l = pd.DataFrame(data=data_low, index=bars_bid.index.shift(periods=-100, freq="ms"))
        df_ticks_c = pd.DataFrame(data=data_close)

        # Drop rows with no volume
        df_ticks_o = df_ticks_o[(df_ticks_h[["bid_size"]] > 0).all(axis=1)]
        df_ticks_h = df_ticks_h[(df_ticks_h[["bid_size"]] > 0).all(axis=1)]
        df_ticks_l = df_ticks_l[(df_ticks_l[["bid_size"]] > 0).all(axis=1)]
        df_ticks_c = df_ticks_c[(df_ticks_c[["bid_size"]] > 0).all(axis=1)]
        df_ticks_o = df_ticks_o[(df_ticks_h[["ask_size"]] > 0).all(axis=1)]
        df_ticks_h = df_ticks_h[(df_ticks_h[["ask_size"]] > 0).all(axis=1)]
        df_ticks_l = df_ticks_l[(df_ticks_l[["ask_size"]] > 0).all(axis=1)]
        df_ticks_c = df_ticks_c[(df_ticks_c[["ask_size"]] > 0).all(axis=1)]

        # Set high low tick volumes to zero
        df_ticks_o["bid_size"] = 0
        df_ticks_o["ask_size"] = 0
        df_ticks_h["bid_size"] = 0
        df_ticks_h["ask_size"] = 0
        df_ticks_l["bid_size"] = 0
        df_ticks_l["ask_size"] = 0

        # Merge tick data
        df_ticks_final = pd.concat([df_ticks_o, df_ticks_h, df_ticks_l, df_ticks_c])
        df_ticks_final.sort_index(axis=0, kind="mergesort", inplace=True)

        # Build ticks from data
        self.tick_data = df_ticks_final
        self.tick_data["symbol"] = symbol_indexer

    cpdef QuoteTick _build_tick_from_values_with_sizes(self, double[:] values, datetime timestamp):
        """
        Build a tick from the given values. The function expects the values to
        be an ndarray with 4 elements [bid, ask, bid_size, ask_size] of type double.
        """
        return QuoteTick(
            self.instrument.symbol,
            Price(values[0], self.instrument.price_precision),
            Price(values[1], self.instrument.price_precision),
            Quantity(values[2], self.instrument.size_precision),
            Quantity(values[3], self.instrument.size_precision),
            timestamp)

    cpdef QuoteTick _build_tick_from_values(self, double[:] values, datetime timestamp):
        """
        Build a tick from the given values. The function expects the values to
        be an ndarray with 4 elements [bid, ask, bid_size, ask_size] of type double.
        """
        return QuoteTick(
            self.instrument.symbol,
            Price(values[0], self.instrument.price_precision),
            Price(values[1], self.instrument.price_precision),
            Quantity.one(),
            Quantity.one(),
            timestamp)


cdef class BarDataWrangler:
    """
    Provides a means of building lists of bars from a given Pandas DataFrame of
    the correct specification.
    """

    def __init__(self,
                 int precision,
                 int volume_multiple=1,
                 data: pd.DataFrame=None):
        """
        Initialize a new instance of the BarDataWrangler class.

        :param precision: The decimal precision for bar prices (>= 0).
        :param data: The the bars market data.
        :param volume_multiple: The volume multiple for the builder (> 0).
        :raises: ValueError: If the decimal_precision is negative (< 0).
        :raises: ValueError: If the volume_multiple is not positive (> 0).
        :raises: ValueError: If the data is a type other than DataFrame.
        """
        Condition.not_negative_int(precision, "precision")
        Condition.positive_int(volume_multiple, "volume_multiple")
        Condition.type(data, pd.DataFrame, "data")

        self._precision = precision
        self._volume_multiple = volume_multiple
        self._data = as_utc_index(data)

    cpdef list build_bars_all(self):
        """
        Return a list of Bars from all data.

        :return List[Bar].
        """
        return list(map(self._build_bar,
                        self._data.values,
                        pd.to_datetime(self._data.index)))

    cpdef list build_bars_from(self, int index=0):
        """
        Return a list of Bars from the given index (>= 0).

        :return List[Bar].
        """
        Condition.not_negative_int(index, "index")

        return list(map(self._build_bar,
                        self._data.iloc[index:].values,
                        pd.to_datetime(self._data.iloc[index:].index)))

    cpdef list build_bars_range(self, int start=0, int end=-1):
        """
        Return a list of Bars within the given range.

        :return List[Bar].
        """
        Condition.not_negative_int(start, "start")

        return list(map(self._build_bar,
                        self._data.iloc[start:end].values,
                        pd.to_datetime(self._data.iloc[start:end].index)))

    cpdef Bar _build_bar(self, double[:] values, datetime timestamp):
        # Build a bar from the given index and values. The function expects the
        # values to be an ndarray with 5 elements [open, high, low, close, volume].
        return Bar(Price(values[0], self._precision),
                   Price(values[1], self._precision),
                   Price(values[2], self._precision),
                   Price(values[3], self._precision),
                   Quantity(values[4] * self._volume_multiple),
                   timestamp)


cdef class BarBuilder:
    """
    The base class for all bar builders.
    """

    def __init__(self, BarSpecification bar_spec not None, bint use_previous_close=False):
        """
        Initialize a new instance of the BarBuilder class.

        :param bar_spec: The bar specification for the builder.
        :param use_previous_close: The flag indicating whether the previous close
        price should be the open price of a new bar.
        """
        self.bar_spec = bar_spec
        self.last_update = None
        self.initialized = False
        self.use_previous_close = use_previous_close
        self.count = 0

        self._last_close = None
        self._open = None
        self._high = None
        self._low = None
        self._close = None
        self._volume = Quantity.zero()

    cpdef void update(self, QuoteTick tick) except *:
        """
        Update the builder with the given tick.

        :param tick: The tick to update with.
        """
        Condition.not_none(tick, "tick")

        if self.last_update is not None and tick.timestamp < self.last_update:
            return  # Previously handled tick

        cdef Price price = self._get_price(tick)

        if self._open is None:
            # Initialize builder
            self._open = price
            self._high = price
            self._low = price
            self.initialized = True
        elif price.gt(self._high):
            self._high = price
        elif price.lt(self._low):
            self._low = price

        self._close = price
        self._volume = self._get_volume(tick)
        self.count += 1
        self.last_update = tick.timestamp

    cpdef Bar build(self, datetime close_time=None):
        """
        Return a bar from the internal properties.

        :param close_time: The optional closing time for the bar (if None will be last updated time).

        :return: Bar.
        """
        if close_time is None:
            close_time = self.last_update

        if self._open is None:  # No tick was received
            self._open = self._last_close
            self._high = self._last_close
            self._low = self._last_close
            self._close = self._last_close

        cdef Bar bar = Bar(
            open_price=self._open,
            high_price=self._high,
            low_price=self._low,
            close_price=self._close,
            volume=self._volume,
            timestamp=close_time)

        self._last_close = self._close
        self._reset()
        return bar

    cdef void _reset(self) except *:
        if self.use_previous_close:
            self._open = self._close
            self._high = self._close
            self._low = self._close
        else:
            self._open = None
            self._high = None
            self._low = None
            self._close = None

        self._volume = Quantity.zero()
        self.count = 0

    cdef Price _get_price(self, QuoteTick tick):
        if self.bar_spec.price_type == PriceType.MID:
            return Price((tick.bid.as_double() + tick.ask.as_double()) / 2, tick.bid.precision + 1)
        elif self.bar_spec.price_type == PriceType.BID:
            return tick.bid
        elif self.bar_spec.price_type == PriceType.ASK:
            return tick.ask
        else:
            raise ValueError(f"The PriceType {price_type_to_string(self.bar_spec.price_type)} is not supported.")

    cdef Quantity _get_volume(self, QuoteTick tick):
        cdef int max_precision
        cdef double total_volume
        if self.bar_spec.price_type == PriceType.MID:
            max_precision = max(self._volume.precision, tick.bid_size.precision, tick.ask_size.precision)
            total_volume = self._volume.as_double() + ((tick.bid_size + tick.ask_size) / 2.0)
            return self._volume.add(Quantity(total_volume, max_precision))
        elif self.bar_spec.price_type == PriceType.BID:
            return self._volume.add(tick.bid_size)
        elif self.bar_spec.price_type == PriceType.ASK:
            return self._volume.add(tick.ask_size)
        else:
            raise ValueError(f"The PriceType {price_type_to_string(self.bar_spec.price_type)} is not supported.")

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return f"BarBuilder(bar_spec={self.bar_spec},{self._open},{self._high},{self._low},{self._close},{self._volume})"

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        Returns
        -------
        str

        """
        return f"<{str(self)} object at {id(self)}>"


cdef class BarAggregator:
    """
    Provides a means of aggregating built bars to the registered handler.
    """

    def __init__(self,
                 BarType bar_type not None,
                 handler not None,
                 Logger logger not None,
                 bint use_previous_close):
        """
        Initialize a new instance of the BarAggregator class.

        :param bar_type: The bar type for the aggregator.
        :param handler: The bar handler for the aggregator.
        :param logger: The logger for the aggregator.
        :param use_previous_close: If the previous close price should be the open price of a new bar.
        """
        self.bar_type = bar_type
        self._handler = BarHandler(handler)
        self._log = LoggerAdapter(self.__class__.__name__, logger)
        self._builder = BarBuilder(
            bar_spec=self.bar_type.spec,
            use_previous_close=use_previous_close)

    cpdef void update(self, QuoteTick tick) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void _handle_bar(self, Bar bar) except *:
        self._handler.handle(self.bar_type, bar)


cdef class TickBarAggregator(BarAggregator):
    """
    Provides a means of building tick bars from ticks.
    """

    def __init__(self,
                 BarType bar_type not None,
                 handler not None,
                 Logger logger not None):
        """
        Initialize a new instance of the TickBarBuilder class.

        :param bar_type: The bar type for the aggregator.
        :param handler: The bar handler for the aggregator.
        :param logger: The logger for the aggregator.
        """
        super().__init__(bar_type=bar_type,
                         handler=handler,
                         logger=logger,
                         use_previous_close=False)

        self.step = bar_type.spec.step

    cpdef void update(self, QuoteTick tick) except *:
        """
        Update the builder with the given tick.

        :param tick: The tick for the update.
        """
        Condition.not_none(tick, "tick")

        self._builder.update(tick)

        cdef Bar bar
        if self._builder.count == self.step:
            try:
                bar = self._builder.build()
            except ValueError as ex:
                # Bar was somehow malformed
                self._log.exception(ex)
                return

            self._handle_bar(bar)


cdef class TimeBarAggregator(BarAggregator):
    """
    Provides a means of building time bars from ticks with an internal timer.
    """
    def __init__(self,
                 BarType bar_type not None,
                 handler not None,
                 bint use_previous_close,
                 Clock clock not None,
                 Logger logger not None):
        """
        Initialize a new instance of the TimeBarAggregator class.

        :param bar_type: The bar type for the aggregator.
        :param handler: The bar handler for the aggregator.
        :param use_previous_close: The flag indicating whether the previous close
        should become the next open.
        :param clock: If the clock for the aggregator.
        :param logger: The logger for the aggregator.
        """
        super().__init__(bar_type=bar_type,
                         handler=handler,
                         logger=logger,
                         use_previous_close=use_previous_close)

        self._clock = clock
        self.interval = self._get_interval()
        self._set_build_timer()
        self.next_close = self._clock.get_timer(self.bar_type.to_string()).next_time

    cpdef void update(self, QuoteTick tick) except *:
        """
        Update the builder with the given tick.

        :param tick: The tick for the update.
        """
        Condition.not_none(tick, "tick")

        if self._clock.is_test_clock:
            if self.next_close < tick.timestamp:
                # Build bar first, then update
                self._build_bar(self.next_close)
                self._builder.update(tick)
                return
            elif self.next_close == tick.timestamp:
                # Update first, then build bar
                self._builder.update(tick)
                self._build_bar(self.next_close)
                return

        self._builder.update(tick)

    cpdef void stop(self) except *:
        """
        Stop the bar aggregator.
        """
        self._clock.cancel_timer(self.bar_type.to_string())

    cpdef datetime get_start_time(self):
        cdef datetime now = self._clock.time_now()
        if self.bar_type.spec.aggregation == BarAggregation.SECOND:
            return datetime(
                year=now.year,
                month=now.month,
                day=now.day,
                hour=now.hour,
                minute=now.minute,
                second=now.second,
                tzinfo=now.tzinfo)
        elif self.bar_type.spec.aggregation == BarAggregation.MINUTE:
            return datetime(
                year=now.year,
                month=now.month,
                day=now.day,
                hour=now.hour,
                minute=now.minute,
                tzinfo=now.tzinfo)
        elif self.bar_type.spec.aggregation == BarAggregation.HOUR:
            return datetime(
                year=now.year,
                month=now.month,
                day=now.day,
                hour=now.hour,
                tzinfo=now.tzinfo)
        elif self.bar_type.spec.aggregation == BarAggregation.DAY:
            return datetime(
                year=now.year,
                month=now.month,
                day=now.day)
        else:
            raise ValueError(f"The BarAggregation {bar_aggregation_to_string(self.bar_type.spec.aggregation)} is not supported.")

    cdef timedelta _get_interval(self):
        if self.bar_type.spec.aggregation == BarAggregation.SECOND:
            return timedelta(seconds=(1 * self.bar_type.spec.step))
        elif self.bar_type.spec.aggregation == BarAggregation.MINUTE:
            return timedelta(minutes=(1 * self.bar_type.spec.step))
        elif self.bar_type.spec.aggregation == BarAggregation.HOUR:
            return timedelta(hours=(1 * self.bar_type.spec.step))
        elif self.bar_type.spec.aggregation == BarAggregation.DAY:
            return timedelta(days=(1 * self.bar_type.spec.step))
        else:
            raise ValueError(f"The BarAggregation {bar_aggregation_to_string(self.bar_type.spec.aggregation)} is not supported.")

    cpdef void _set_build_timer(self) except *:
        cdef str timer_name = self.bar_type.to_string()

        self._clock.set_timer(
            name=timer_name,
            interval=self._get_interval(),
            start_time=self.get_start_time(),
            stop_time=None,
            handler=self._build_event)

        self._log.info(f"Started timer {timer_name}.")

    cpdef void _build_bar(self, datetime at_time) except *:
        cdef Timer timer = self._clock.get_timer(self.bar_type.to_string())
        cdef TimeEvent event = timer.pop_next_event()
        self._build_event(event)
        self.next_close = timer.next_time

    cpdef void _build_event(self, TimeEvent event) except *:
        cdef Bar bar
        try:
            if self._builder.use_previous_close and not self._builder.initialized:
                self._log.error(f"Cannot build {self.bar_type} (no prices received).")
                return

            bar = self._builder.build(event.timestamp)
        except ValueError as ex:
            # Bar was somehow malformed
            self._log.exception(ex)
            return

        self._handle_bar(bar)
