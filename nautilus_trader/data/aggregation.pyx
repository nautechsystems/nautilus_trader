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

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta
from libc.stdint cimport int64_t

from decimal import Decimal

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.timer cimport TestTimer
from nautilus_trader.common.timer cimport TimeEvent
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport secs_to_nanos
from nautilus_trader.model.bar cimport Bar
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

    def __init__(self, BarType bar_type not None, bint use_previous_close=False):
        """
        Initialize a new instance of the ``BarBuilder`` class.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the builder.
        use_previous_close : bool
            If the previous close price should set the open price of a new bar.

        """
        self._bar_type = bar_type

        self.use_previous_close = use_previous_close
        self.initialized = False
        self.last_timestamp_ns = 0
        self.count = 0

        self._partial_set = False
        self._last_close = None
        self._open = None
        self._high = None
        self._low = None
        self._close = None
        self.volume = Decimal()

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"{self._bar_type},"
                f"{self._open},"
                f"{self._high},"
                f"{self._low},"
                f"{self._close},"
                f"{self.volume})")

    cpdef void set_partial(self, Bar partial_bar) except *:
        """
        Set the initial values for a partially completed bar.

        This method can only be called once per instance.

        Parameters
        ----------
        partial_bar : Bar
            The partial bar with values to set.

        """
        if self._partial_set:
            return  # Already updated

        self._open = partial_bar.open

        if self._high is None or partial_bar.high > self._high:
            self._high = partial_bar.high

        if self._low is None or partial_bar.low < self._low:
            self._low = partial_bar.low

        if self._close is None:
            self._close = partial_bar.close

        self.volume += partial_bar.volume

        if self.last_timestamp_ns == 0:
            self.last_timestamp_ns = partial_bar.ts_recv_ns

        self._partial_set = True
        self.initialized = True

    cpdef void update(self, Price price, Quantity size, int64_t timestamp_ns) except *:
        """
        Update the bar builder.

        Parameters
        ----------
        price : Price
            The update price.
        size : Decimal
            The update size.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the update.

        """
        Condition.not_none(price, "price")
        Condition.not_none(size, "size")

        # TODO: What happens if the first tick updates before a partial bar is applied?
        if timestamp_ns < self.last_timestamp_ns:
            return  # Not applicable

        if self._open is None:
            # Initialize builder
            self._open = price
            self._high = price
            self._low = price
            self.initialized = True
        elif price > self._high:
            self._high = price
        elif price < self._low:
            self._low = price

        self._close = price
        self.volume += size
        self.count += 1
        self.last_timestamp_ns = timestamp_ns

    cpdef void reset(self) except *:
        """
        Reset the bar builder.

        All stateful fields are reset to their initial value.
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

        self.volume = Decimal()
        self.count = 0

    cpdef Bar build_now(self):
        """
        Return the aggregated bar and reset.

        Returns
        -------
        Bar

        """
        return self.build(self.last_timestamp_ns)

    cpdef Bar build(self, int64_t timestamp_ns):
        """
        Return the aggregated bar with the given closing timestamp, and reset.

        Parameters
        ----------
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the bar close.

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
            bar_type=self._bar_type,
            open_price=self._open,
            high_price=self._high,
            low_price=self._low,
            close_price=self._close,
            volume=Quantity.from_str_c(str(self.volume)),  # TODO: Refactor when precision available
            ts_event_ns=timestamp_ns,  # TODO: Hardcoded identical for now...
            ts_recv_ns=timestamp_ns,
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
        handler not None: callable,
        Logger logger not None,
        bint use_previous_close,
    ):
        """
        Initialize a new instance of the ``BarAggregator`` class.

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
        self._log = LoggerAdapter(
            component=type(self).__name__,
            logger=logger,
        )
        self._builder = BarBuilder(
            bar_type=self.bar_type,
            use_previous_close=use_previous_close,
        )

    cpdef void handle_quote_tick(self, QuoteTick tick) except *:
        """
        Update the aggregator with the given tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick for the update.

        """
        Condition.not_none(tick, "tick")

        self._apply_update(
            price=tick.extract_price(self.bar_type.spec.price_type),
            size=tick.extract_volume(self.bar_type.spec.price_type),
            timestamp_ns=tick.ts_recv_ns,
        )

    cpdef void handle_trade_tick(self, TradeTick tick) except *:
        """
        Update the aggregator with the given tick.

        Parameters
        ----------
        tick : TradeTick
            The tick for the update.

        """
        Condition.not_none(tick, "tick")

        self._apply_update(
            price=tick.price,
            size=tick.size,
            timestamp_ns=tick.ts_recv_ns,
        )

    cdef void _apply_update(self, Price price, Quantity size, int64_t timestamp_ns) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cdef void _build_now_and_send(self) except *:
        cdef Bar bar = self._builder.build_now()
        self._handler(bar)

    cdef void _build_and_send(self, int64_t timestamp_ns) except *:
        cdef Bar bar = self._builder.build(timestamp_ns)
        self._handler(bar)


cdef class TickBarAggregator(BarAggregator):
    """
    Provides a means of building tick bars from ticks.

    When received tick count reaches the step threshold of the bar
    specification, then a bar is created and sent to the handler.
    """

    def __init__(
        self,
        BarType bar_type not None,
        handler not None: callable,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the ``TickBarAggregator`` class.

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

    cdef void _apply_update(self, Price price, Quantity size, int64_t timestamp_ns) except *:
        self._builder.update(price, size, timestamp_ns)

        if self._builder.count == self.bar_type.spec.step:
            self._build_now_and_send()


cdef class VolumeBarAggregator(BarAggregator):
    """
    Provides a means of building volume bars from ticks.

    When received volume reaches the step threshold of the bar
    specification, then a bar is created and sent to the handler.
    """

    def __init__(
        self,
        BarType bar_type not None,
        handler not None: callable,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the ``TickBarAggregator`` class.

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

    cdef void _apply_update(self, Price price, Quantity size, int64_t timestamp_ns) except *:
        size_update = size

        while size_update > 0:  # While there is size to apply
            if self._builder.volume + size_update < self.bar_type.spec.step:
                # Update and break
                self._builder.update(
                    price=price,
                    size=Quantity(size_update, precision=size.precision),
                    timestamp_ns=timestamp_ns,
                )
                break

            size_diff: Decimal = self.bar_type.spec.step - self._builder.volume
            # Update builder to the step threshold
            self._builder.update(
                price=price,
                size=Quantity(size_diff, precision=size.precision),
                timestamp_ns=timestamp_ns,
            )

            # Build a bar and reset builder
            self._build_now_and_send()

            # Decrement the update size
            size_update -= size_diff
            assert size_update >= 0


cdef class ValueBarAggregator(BarAggregator):
    """
    Provides a means of building value bars from ticks.

    When received value reaches the step threshold of the bar
    specification, then a bar is created and sent to the handler.
    """

    def __init__(
        self,
        BarType bar_type not None,
        handler not None: callable,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the ``TickBarAggregator`` class.

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

        self._cum_value = Decimal()  # Cumulative value

    cpdef object get_cumulative_value(self):
        """
        Return the current cumulative value of the aggregator.

        Returns
        -------
        Decimal

        """
        return self._cum_value

    cdef void _apply_update(self, Price price, Quantity size, int64_t timestamp_ns) except *:
        size_update = size

        while size_update > 0:  # While there is value to apply
            value_update = price * size_update  # Calculated value in quote currency
            if self._cum_value + value_update < self.bar_type.spec.step:
                # Update and break
                self._cum_value = self._cum_value + value_update
                self._builder.update(
                    price=price,
                    size=Quantity(size_update, precision=size.precision),
                    timestamp_ns=timestamp_ns,
                )
                break

            value_diff: Decimal = self.bar_type.spec.step - self._cum_value
            size_diff: Decimal = size_update * (value_diff / value_update)
            # Update builder to the step threshold
            self._builder.update(
                price=price,
                size=Quantity(size_diff, precision=size.precision),
                timestamp_ns=timestamp_ns,
            )

            # Build a bar and reset builder and cumulative value
            self._build_now_and_send()
            self._cum_value = Decimal()

            # Decrement the update size
            size_update -= size_diff
            assert size_update >= 0


cdef class TimeBarAggregator(BarAggregator):
    """
    Provides a means of building time bars from ticks with an internal timer.

    When the time reaches the next time interval of the bar specification, then
    a bar is created and sent to the handler.
    """
    def __init__(
        self,
        BarType bar_type not None,
        handler not None: callable,
        bint use_previous_close,
        Clock clock not None,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the ``TimeBarAggregator`` class.

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
        self.interval_ns = self._get_interval_ns()
        self._set_build_timer()
        self.next_close_ns = self._clock.timer(str(self.bar_type)).next_time_ns
        self._build_on_next_tick = False
        self._stored_close_ns = 0

    cpdef datetime get_start_time(self):
        """
        Return the start time for the aggregators next bar.

        Returns
        -------
        datetime
            The timestamp (UTC).

        """
        cdef datetime now = self._clock.utc_now()
        cdef int step = self.bar_type.spec.step

        cdef datetime start_time
        if self.bar_type.spec.aggregation == BarAggregation.SECOND:
            start_time = now - timedelta(
                seconds=now.second % step,
                microseconds=now.microsecond,
            )
        elif self.bar_type.spec.aggregation == BarAggregation.MINUTE:
            start_time = now - timedelta(
                minutes=now.minute % step,
                seconds=now.second,
                microseconds=now.microsecond,
            )
        elif self.bar_type.spec.aggregation == BarAggregation.HOUR:
            start_time = now - timedelta(
                hours=now.hour % step,
                minutes=now.minute,
                seconds=now.second,
                microseconds=now.microsecond,
            )
        elif self.bar_type.spec.aggregation == BarAggregation.DAY:
            start_time = now - timedelta(
                days=now.day % step,
                hours=now.hour,
                minutes=now.minute,
                seconds=now.second,
                microseconds=now.microsecond,
            )
        else:
            # Design time error
            raise ValueError(f"Aggregation not a time, "
                             f"was {BarAggregationParser.to_str(self.bar_type.spec.aggregation)}")

        return start_time

    cpdef void set_partial(self, Bar partial_bar) except *:
        """
        Set the initial values for a partially completed bar.

        This method can only be called once per instance.

        Parameters
        ----------
        partial_bar : Bar
            The partial bar with values to set.

        """
        self._builder.set_partial(partial_bar)

    cpdef void stop(self) except *:
        """
        Stop the bar aggregator.
        """
        self._clock.cancel_timer(str(self.bar_type))

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
                             f"was {BarAggregationParser.to_str(aggregation)}")

    cdef int64_t _get_interval_ns(self):
        cdef BarAggregation aggregation = self.bar_type.spec.aggregation
        cdef int step = self.bar_type.spec.step

        if aggregation == BarAggregation.SECOND:
            return secs_to_nanos(step)
        elif aggregation == BarAggregation.MINUTE:
            return secs_to_nanos(step) * 60
        elif aggregation == BarAggregation.HOUR:
            return secs_to_nanos(step) * 60 * 60
        elif aggregation == BarAggregation.DAY:
            return secs_to_nanos(step) * 60 * 60 * 24
        else:
            # Design time error
            raise ValueError(f"Aggregation not time range, "
                             f"was {BarAggregationParser.to_str(aggregation)}")

    cpdef void _set_build_timer(self) except *:
        cdef str timer_name = str(self.bar_type)

        self._clock.set_timer(
            name=timer_name,
            interval=self.interval,
            start_time=self.get_start_time(),
            stop_time=None,
            handler=self._build_event,
        )

        self._log.debug(f"Started timer {timer_name}.")

    cdef void _apply_update(self, Price price, Quantity size, int64_t timestamp_ns) except *:
        if self._clock.is_test_clock:
            if self.next_close_ns < timestamp_ns:
                # Build bar first, then update
                self._build_bar(self.next_close_ns)
                self._builder.update(price, size, timestamp_ns)
                return
            elif self.next_close_ns == timestamp_ns:
                # Update first, then build bar
                self._builder.update(price, size, timestamp_ns)
                self._build_bar(self.next_close_ns)
                return

        self._builder.update(price, size, timestamp_ns)
        if self._build_on_next_tick:  # (fast C-level check)
            self._build_and_send(self._stored_close)
            # Reset flag and clear stored close
            self._build_on_next_tick = False
            self._stored_close = 0

    cpdef void _build_bar(self, int64_t timestamp_ns) except *:
        cdef TestTimer timer = self._clock.timer(str(self.bar_type))
        cdef TimeEvent event = timer.pop_next_event()
        self._build_event(event)
        self.next_close_ns = timer.next_time_ns

    cpdef void _build_event(self, TimeEvent event) except *:
        if self._builder.use_previous_close and not self._builder.initialized:
            # Set flag to build on next close with the stored close time
            self._build_on_next_tick = True
            self._stored_close_ns = self.next_close_ns
            return

        self._build_and_send(timestamp_ns=event.event_timestamp_ns)


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
        Initialize a new instance of the ``BulkTickBarBuilder`` class.

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
        self.aggregator = TickBarAggregator(bar_type, self.bars.append, logger)
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

        self.callback(self.bars)


cdef class BulkTimeBarUpdater:
    """
    Provides a temporary updater for time bars from a bulk tick order.
    """

    def __init__(self, TimeBarAggregator aggregator not None):
        """
        Initialize a new instance of the ``BulkTimeBarUpdater`` class.

        Parameters
        ----------
        aggregator : TimeBarAggregator
            The time bar aggregator to update.

        """
        self.aggregator = aggregator
        self.start_time_ns = self.aggregator.next_close_ns - self.aggregator.interval_ns

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
                if ticks[i].timestamp_ns < self.start_time_ns:
                    continue  # Price not applicable to this bar
                self.aggregator.handle_trade_tick(ticks[i])
        else:
            for i in range(len(ticks)):
                if ticks[i].timestamp_ns < self.start_time_ns:
                    continue  # Price not applicable to this bar
                self.aggregator.handle_quote_tick(ticks[i])
