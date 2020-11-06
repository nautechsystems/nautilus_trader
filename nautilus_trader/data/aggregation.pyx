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

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta
from cpython.datetime cimport datetime_year
from cpython.datetime cimport datetime_month
from cpython.datetime cimport datetime_day
from cpython.datetime cimport datetime_hour
from cpython.datetime cimport datetime_minute
from cpython.datetime cimport datetime_second
from cpython.datetime cimport datetime_tzinfo

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.timer cimport TimeEvent
from nautilus_trader.common.timer cimport TestTimer
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarData
from nautilus_trader.model.bar cimport BarSpecification
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregationParser
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick


cdef class BarBuilder:
    """
    Provides a generic bar builder for aggregation.
    """

    def __init__(self, BarSpecification bar_spec not None, bint use_previous_close=False):
        """
        Initialize a new instance of the `BarBuilder` class.

        Parameters
        ----------
        bar_spec : BarSpecification
            The bar specification for the builder.
        use_previous_close : bool
            If the previous close price should set the open price of a new bar.

        """
        self.bar_spec = bar_spec
        self.use_previous_close = use_previous_close
        self.initialized = False
        self.last_timestamp = None
        self.count = 0

        self._last_close = None
        self._open = None
        self._high = None
        self._low = None
        self._close = None
        self._volume = Decimal()

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"bar_spec={self.bar_spec},"
                f"{self._open},"
                f"{self._high},"
                f"{self._low},"
                f"{self._close},"
                f"{self._volume})")

    cpdef void update(self, Price price, Decimal volume, datetime timestamp) except *:
        """
        Update the bar builder.

        Parameters
        ----------
        price : Price
            The update price.
        volume : Decimal
            The update volume.
        timestamp : datetime
            The update timestamp.

        """
        Condition.not_none(price, "price")
        Condition.not_none(volume, "volume")
        Condition.not_none(timestamp, "timestamp")

        if self.last_timestamp and timestamp < self.last_timestamp:
            return  # Not applicable

        if self._open is None:
            # Initialize builder
            self._open = price
            self._high = price
            self._low = price
            self.initialized = True
        else:
            self._high = max(self._high, price)
            self._low = min(self._low, price)

        self._close = price
        self._volume = self._volume + volume
        self.count += 1
        self.last_timestamp = timestamp

    cpdef void reset(self) except *:
        """
        Reset the bar builder.

        All stateful values are reset to their initial value.
        """
        if self.use_previous_close:
            self._open = self._close
            self._high = self._close
            self._low = self._close
        else:
            self._open = None
            self._high = None
            self._low = None
            self._close = None

        self._volume = Quantity()
        self.count = 0

    cpdef Bar build(self, datetime close_time=None):
        """
        Return the aggregated bar and reset.

        Parameters
        ----------
        close_time : datetime, optional
            The closing time for the bar (if None will be last updated time).

        Returns
        -------
        Bar

        """
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
            volume=Quantity(self._volume),
            timestamp=close_time if close_time is not None else self.last_timestamp,
        )

        self._last_close = self._close
        self.reset()
        return bar


cdef class BarAggregator:
    """
    Provides a means of aggregating specified bars and sending to a registered handler.
    """

    def __init__(
            self,
            BarType bar_type not None,
            handler not None,
            Logger logger not None,
            bint use_previous_close,
    ):
        """
        Initialize a new instance of the `BarAggregator` class.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the aggregator.
        handler : callable
            The bar handler for the aggregator.
        logger : Logger
            The logger for the aggregator.
        use_previous_close : bool
            If the previous close price should set the open price of a new bar.

        """
        self.bar_type = bar_type
        self._handler = handler
        self._log = LoggerAdapter(type(self).__name__, logger)
        self._builder = BarBuilder(
            bar_spec=self.bar_type.spec,
            use_previous_close=use_previous_close,
        )

    cpdef void handle_quote_tick(self, QuoteTick tick) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void handle_trade_tick(self, TradeTick tick) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cdef void _build_and_send(self, datetime close=None) except *:
        cdef Bar bar = self._builder.build(close)
        cdef BarData data = BarData(self.bar_type, bar)
        self._handler(data)


cdef class TickBarAggregator(BarAggregator):
    """
    Provides a means of building tick bars from ticks.

    When received tick count reaches the step threshold of the bar
    specification, then a bar is created and sent to the handler.
    """

    def __init__(
            self,
            BarType bar_type not None,
            handler not None,
            Logger logger not None,
    ):
        """
        Initialize a new instance of the `TickBarAggregator` class.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the aggregator.
        handler : callable
            The bar handler for the aggregator.
        logger : Logger
            The logger for the aggregator.

        """
        super().__init__(
            bar_type=bar_type,
            handler=handler,
            logger=logger,
            use_previous_close=False,
        )

        self.step = bar_type.spec.step

    cpdef void handle_quote_tick(self, QuoteTick tick) except *:
        """
        Update the builder with the given tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick for the update.

        """
        Condition.not_none(tick, "tick")

        self._builder.update(
            price=tick.extract_price(self._builder.bar_spec.price_type),
            volume=tick.extract_volume(self._builder.bar_spec.price_type),
            timestamp=tick.timestamp,
        )

        self._check_bar_builder()

    cpdef void handle_trade_tick(self, TradeTick tick) except *:
        """
        Update the builder with the given tick.

        Parameters
        ----------
        tick : TradeTick
            The tick for the update.

        """
        Condition.not_none(tick, "tick")

        self._builder.update(
            price=tick.price,
            volume=tick.size,
            timestamp=tick.timestamp,
        )

        self._check_bar_builder()

    cdef inline void _check_bar_builder(self) except *:
        cdef Bar bar
        if self._builder.count == self.step:
            self._build_and_send()


cdef class TimeBarAggregator(BarAggregator):
    """
    Provides a means of building time bars from ticks with an internal timer.

    When the time reaches the next time interval of the bar specification, then
    a bar is created and sent to the handler.
    """
    def __init__(
            self,
            BarType bar_type not None,
            handler not None,
            bint use_previous_close,
            Clock clock not None,
            Logger logger not None,
    ):
        """
        Initialize a new instance of the `TimeBarAggregator` class.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the aggregator.
        handler : callable
            The bar handler for the aggregator.
        use_previous_close : bool
            If the previous close should set the next open.
        clock : Clock
            The clock for the aggregator.
        logger : Logger
            The logger for the aggregator.

        """
        super().__init__(
            bar_type=bar_type,
            handler=handler,
            logger=logger,
            use_previous_close=use_previous_close,
        )

        self._clock = clock
        self.interval = self._get_interval()
        self._set_build_timer()
        self.next_close = self._clock.timer(str(self.bar_type)).next_time

    cpdef void handle_quote_tick(self, QuoteTick tick) except *:
        """
        Update the builder with the given tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick for the update.

        """
        Condition.not_none(tick, "tick")

        cdef Price price = tick.extract_price(self._builder.bar_spec.price_type)
        cdef Quantity volume = tick.extract_volume(self._builder.bar_spec.price_type)

        if self._clock.is_test_clock:
            if self.next_close < tick.timestamp:
                # Build bar first, then update
                self._build_bar(self.next_close)
                self._builder.update(price, volume, tick.timestamp)
                return
            elif self.next_close == tick.timestamp:
                # Update first, then build bar
                self._builder.update(price, volume, tick.timestamp)
                self._build_bar(self.next_close)
                return

        self._builder.update(price, volume, tick.timestamp)

    cpdef void handle_trade_tick(self, TradeTick tick) except *:
        """
        Update the builder with the given tick.

        Parameters
        ----------
        tick : TradeTick
            The tick for the update.

        """
        Condition.not_none(tick, "tick")

        if self._clock.is_test_clock:
            if self.next_close < tick.timestamp:
                # Build bar first, then update
                self._build_bar(self.next_close)
                self._builder.update(tick.price, tick.size, tick.timestamp)
                return
            elif self.next_close == tick.timestamp:
                # Update first, then build bar
                self._builder.update(tick.price, tick.size, tick.timestamp)
                self._build_bar(self.next_close)
                return

        self._builder.update(tick.price, tick.size, tick.timestamp)

    cpdef void stop(self) except *:
        """
        Stop the bar aggregator.
        """
        self._clock.cancel_timer(str(self.bar_type))

    cpdef datetime get_start_time(self):
        cdef datetime now = self._clock.utc_now()
        if self.bar_type.spec.aggregation == BarAggregation.SECOND:
            return datetime(
                year=datetime_year(now),
                month=datetime_month(now),
                day=datetime_day(now),
                hour=datetime_hour(now),
                minute=datetime_minute(now),
                second=datetime_second(now),
                tzinfo=datetime_tzinfo(now),
            )
        elif self.bar_type.spec.aggregation == BarAggregation.MINUTE:
            return datetime(
                year=datetime_year(now),
                month=datetime_month(now),
                day=datetime_day(now),
                hour=datetime_hour(now),
                minute=datetime_minute(now),
                tzinfo=datetime_tzinfo(now),
            )
        elif self.bar_type.spec.aggregation == BarAggregation.HOUR:
            return datetime(
                year=datetime_year(now),
                month=datetime_month(now),
                day=datetime_day(now),
                hour=datetime_hour(now),
                tzinfo=datetime_tzinfo(now),
            )
        elif self.bar_type.spec.aggregation == BarAggregation.DAY:
            return datetime(
                year=datetime_year(now),
                month=datetime_month(now),
                day=datetime_day(now),
            )
        else:
            # Design time error
            raise ValueError(f"Aggregation not a time, "
                             f"was {BarAggregationParser.to_string(self.bar_type.spec.aggregation)}")

    cdef timedelta _get_interval(self):
        cdef BarAggregation aggregation = self.bar_type.spec.aggregation
        cdef int step = self.bar_type.spec.step

        if aggregation == BarAggregation.SECOND:
            return timedelta(seconds=(1 * step))
        elif aggregation == BarAggregation.MINUTE:
            return timedelta(minutes=(1 * step))
        elif aggregation == BarAggregation.HOUR:
            return timedelta(hours=(1 * step))
        elif aggregation == BarAggregation.DAY:
            return timedelta(days=(1 * step))
        else:
            # Design time error
            raise ValueError(f"Aggregation not time range, "
                             f"was {BarAggregationParser.to_string(aggregation)}")

    cpdef void _set_build_timer(self) except *:
        cdef str timer_name = str(self.bar_type)

        self._clock.set_timer(
            name=timer_name,
            interval=self._get_interval(),
            start_time=self.get_start_time(),
            stop_time=None,
            handler=self._build_event,
        )

        self._log.info(f"Started timer {timer_name}.")

    cpdef void _build_bar(self, datetime at_time) except *:
        cdef TestTimer timer = self._clock.timer(str(self.bar_type))
        cdef TimeEvent event = timer.pop_next_event()
        self._build_event(event)
        self.next_close = timer.next_time

    cpdef void _build_event(self, TimeEvent event) except *:
        if self._builder.use_previous_close and not self._builder.initialized:
            self._log.error(f"Cannot build {self.bar_type} (no prices received).")
            return

        self._build_and_send(close=event.timestamp)


cdef class BulkTickBarBuilder:
    """
    Provides a temporary builder for tick bars from a bulk tick order.
    """

    def __init__(
            self,
            BarType bar_type not None,
            Logger logger not None,
            callback not None: callable,
    ):
        """
        Initialize a new instance of the `BulkTickBarBuilder` class.

        Parameters
        ----------
        bar_type : BarType
            The bar_type to build.
        logger : Logger
            The logger for the bar aggregator.
        callback : callable
            The callback to send the built bars to.

        Raises
        ------
        ValueError
            If callback is not of type callable.

        """
        Condition.callable(callback, "callback")

        self.bars = []
        self.aggregator = TickBarAggregator(bar_type, self._add_bar, logger)
        self.callback = callback

    def receive(self, list ticks):
        """
        Receive the bulk list of ticks and build aggregated bars.

        Then send the bar type and bars list on to the registered callback.

        Parameters
        ----------
        ticks : list[Tick]
            The ticks for aggregation.

        """
        Condition.not_none(ticks, "ticks")

        if self.aggregator.bar_type.spec.price_type == PriceType.LAST:
            for i in range(len(ticks)):
                self.aggregator.handle_trade_tick(ticks[i])
        else:
            for i in range(len(ticks)):
                self.aggregator.handle_quote_tick(ticks[i])

        self.callback(self.aggregator.bar_type, self.bars)

    cpdef void _add_bar(self, BarData data) except *:
        self.bars.append(data.bar)


cdef class BulkTimeBarUpdater:
    """
    Provides a temporary updater for time bars from a bulk tick order.
    """

    def __init__(self, TimeBarAggregator aggregator not None):
        """
        Initialize a new instance of the `BulkTimeBarUpdater` class.

        Parameters
        ----------
        aggregator : TimeBarAggregator
            The time bar aggregator to update.

        """
        self.aggregator = aggregator
        self.start_time = self.aggregator.next_close - self.aggregator.interval

    def receive(self, list ticks):
        """
        Receive the bulk list of ticks and update the aggregator.

        Parameters
        ----------
        ticks : list[Tick]
            The ticks for updating.

        """
        if self.aggregator.bar_type.spec.price_type == PriceType.LAST:
            for i in range(len(ticks)):
                # noinspection PyUnresolvedReferences
                if ticks[i].timestamp < self.start_time:
                    continue  # Price not applicable to this bar
                self.aggregator.handle_trade_tick(ticks[i])
        else:
            for i in range(len(ticks)):
                # noinspection PyUnresolvedReferences
                if ticks[i].timestamp < self.start_time:
                    continue  # Price not applicable to this bar
                self.aggregator.handle_quote_tick(ticks[i])
