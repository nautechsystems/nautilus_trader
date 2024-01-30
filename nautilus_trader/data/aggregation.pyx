# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal
from typing import Callable

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta
from libc.stdint cimport uint64_t

from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport TimeEvent
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport dt_to_unix_nanos
from nautilus_trader.core.rust.core cimport millis_to_nanos
from nautilus_trader.core.rust.core cimport secs_to_nanos
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport BarAggregation
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.functions cimport bar_aggregation_to_str
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class BarBuilder:
    """
    Provides a generic bar builder for aggregation.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the builder.
    bar_type : BarType
        The bar type for the builder.

    Raises
    ------
    ValueError
        If `instrument.id` != `bar_type.instrument_id`.
    """

    def __init__(
        self,
        Instrument instrument not None,
        BarType bar_type not None,
    ):
        Condition.equal(instrument.id, bar_type.instrument_id, "instrument.id", "bar_type.instrument_id")

        self._bar_type = bar_type

        self.price_precision = instrument.price_precision
        self.size_precision = instrument.size_precision
        self.initialized = False
        self.ts_last = 0
        self.count = 0

        self._partial_set = False
        self._last_close = None
        self._open = None
        self._high = None
        self._low = None
        self._close = None
        self.volume = Quantity.zero_c(precision=self.size_precision)

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"{self._bar_type},"
            f"{self._open},"
            f"{self._high},"
            f"{self._low},"
            f"{self._close},"
            f"{self.volume})"
        )

    cpdef void set_partial(self, Bar partial_bar):
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

        self.volume = partial_bar.volume

        if self.ts_last == 0:
            self.ts_last = partial_bar.ts_init

        self._partial_set = True
        self.initialized = True

    cpdef void update(self, Price price, Quantity size, uint64_t ts_event):
        """
        Update the bar builder.

        Parameters
        ----------
        price : Price
            The update price.
        size : Decimal
            The update size.
        ts_event : uint64_t
            The UNIX timestamp (nanoseconds) of the update.

        """
        Condition.not_none(price, "price")
        Condition.not_none(size, "size")

        # TODO: What happens if the first tick updates before a partial bar is applied?
        if ts_event < self.ts_last:
            return  # Not applicable

        if self._open is None:
            # Initialize builder
            self._open = price
            self._high = price
            self._low = price
            self.initialized = True
        elif price._mem.raw > self._high._mem.raw:
            self._high = price
        elif price._mem.raw < self._low._mem.raw:
            self._low = price

        self._close = price
        self.volume._mem.raw += size._mem.raw
        self.count += 1
        self.ts_last = ts_event

    cpdef void reset(self):
        """
        Reset the bar builder.

        All stateful fields are reset to their initial value.
        """
        self._open = None
        self._high = None
        self._low = None

        self.volume = Quantity.zero_c(precision=self.size_precision)
        self.count = 0

    cpdef Bar build_now(self):
        """
        Return the aggregated bar and reset.

        Returns
        -------
        Bar

        """
        return self.build(self.ts_last, self.ts_last)

    cpdef Bar build(self, uint64_t ts_event, uint64_t ts_init):
        """
        Return the aggregated bar with the given closing timestamp, and reset.

        Parameters
        ----------
        ts_event : uint64_t
            The UNIX timestamp (nanoseconds) for the bar event.
        ts_init : uint64_t
            The UNIX timestamp (nanoseconds) for the bar initialization.

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
            open=self._open,
            high=self._high,
            low=self._low,
            close=self._close,
            volume=Quantity(self.volume, self.size_precision),
            ts_event=ts_event,
            ts_init=ts_init,
        )

        self._last_close = self._close
        self.reset()
        return bar


cdef class BarAggregator:
    """
    Provides a means of aggregating specified bars and sending to a registered handler.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the aggregator.
    bar_type : BarType
        The bar type for the aggregator.
    handler : Callable[[Bar], None]
        The bar handler for the aggregator.
    await_partial : bool, default False
        If the aggregator should await an initial partial bar prior to aggregating.

    Raises
    ------
    ValueError
        If `instrument.id` != `bar_type.instrument_id`.
    """

    def __init__(
        self,
        Instrument instrument not None,
        BarType bar_type not None,
        handler not None: Callable[[Bar], None],
        bint await_partial = False,
    ):
        Condition.equal(instrument.id, bar_type.instrument_id, "instrument.id", "bar_type.instrument_id")

        self.bar_type = bar_type
        self._handler = handler
        self._await_partial = await_partial
        self._log = Logger(name=type(self).__name__)
        self._builder = BarBuilder(
            instrument=instrument,
            bar_type=self.bar_type,
        )

    def set_await_partial(self, bint value):
        self._await_partial = value

    cpdef void handle_quote_tick(self, QuoteTick tick):
        """
        Update the aggregator with the given tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick for the update.

        """
        Condition.not_none(tick, "tick")

        if not self._await_partial:
            self._apply_update(
                price=tick.extract_price(self.bar_type.spec.price_type),
                size=tick.extract_volume(self.bar_type.spec.price_type),
                ts_event=tick.ts_event,
            )

    cpdef void handle_trade_tick(self, TradeTick tick):
        """
        Update the aggregator with the given tick.

        Parameters
        ----------
        tick : TradeTick
            The tick for the update.

        """
        Condition.not_none(tick, "tick")

        if not self._await_partial:
            self._apply_update(
                price=tick.price,
                size=tick.size,
                ts_event=tick.ts_event,
            )

    cpdef void set_partial(self, Bar partial_bar):
        """
        Set the initial values for a partially completed bar.

        This method can only be called once per instance.

        Parameters
        ----------
        partial_bar : Bar
            The partial bar with values to set.

        """
        self._builder.set_partial(partial_bar)

    cdef void _apply_update(self, Price price, Quantity size, uint64_t ts_event):
        raise NotImplementedError("method `_apply_update` must be implemented in the subclass")  # pragma: no cover

    cdef void _build_now_and_send(self):
        cdef Bar bar = self._builder.build_now()
        self._handler(bar)

    cdef void _build_and_send(self, uint64_t ts_event, uint64_t ts_init):
        cdef Bar bar = self._builder.build(ts_event=ts_event, ts_init=ts_init)
        self._handler(bar)


cdef class TickBarAggregator(BarAggregator):
    """
    Provides a means of building tick bars from ticks.

    When received tick count reaches the step threshold of the bar
    specification, then a bar is created and sent to the handler.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the aggregator.
    bar_type : BarType
        The bar type for the aggregator.
    handler : Callable[[Bar], None]
        The bar handler for the aggregator.

    Raises
    ------
    ValueError
        If `instrument.id` != `bar_type.instrument_id`.
    """

    def __init__(
        self,
        Instrument instrument not None,
        BarType bar_type not None,
        handler not None: Callable[[Bar], None],
    ):
        super().__init__(
            instrument=instrument,
            bar_type=bar_type,
            handler=handler,
        )

    cdef void _apply_update(self, Price price, Quantity size, uint64_t ts_event):
        self._builder.update(price, size, ts_event)

        if self._builder.count == self.bar_type.spec.step:
            self._build_now_and_send()


cdef class VolumeBarAggregator(BarAggregator):
    """
    Provides a means of building volume bars from ticks.

    When received volume reaches the step threshold of the bar
    specification, then a bar is created and sent to the handler.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the aggregator.
    bar_type : BarType
        The bar type for the aggregator.
    handler : Callable[[Bar], None]
        The bar handler for the aggregator.

    Raises
    ------
    ValueError
        If `instrument.id` != `bar_type.instrument_id`.
    """

    def __init__(
        self,
        Instrument instrument not None,
        BarType bar_type not None,
        handler not None: Callable[[Bar], None],
    ):
        super().__init__(
            instrument=instrument,
            bar_type=bar_type,
            handler=handler,
        )

    cdef void _apply_update(self, Price price, Quantity size, uint64_t ts_event):
        cdef uint64_t raw_size_update = size._mem.raw
        cdef uint64_t raw_step = int(self.bar_type.spec.step * 1e9)
        cdef uint64_t raw_size_diff = 0

        while raw_size_update > 0:  # While there is size to apply
            if self._builder.volume._mem.raw + raw_size_update < raw_step:
                # Update and break
                self._builder.update(
                    price=price,
                    size=Quantity.from_raw_c(raw_size_update, precision=size._mem.precision),
                    ts_event=ts_event,
                )
                break

            raw_size_diff = raw_step - self._builder.volume._mem.raw
            # Update builder to the step threshold
            self._builder.update(
                price=price,
                size=Quantity.from_raw_c(raw_size_diff, precision=size._mem.precision),
                ts_event=ts_event,
            )

            # Build a bar and reset builder
            self._build_now_and_send()

            # Decrement the update size
            raw_size_update -= raw_size_diff
            assert raw_size_update >= 0


cdef class ValueBarAggregator(BarAggregator):
    """
    Provides a means of building value bars from ticks.

    When received value reaches the step threshold of the bar
    specification, then a bar is created and sent to the handler.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the aggregator.
    bar_type : BarType
        The bar type for the aggregator.
    handler : Callable[[Bar], None]
        The bar handler for the aggregator.

    Raises
    ------
    ValueError
        If `instrument.id` != `bar_type.instrument_id`.
    """

    def __init__(
        self,
        Instrument instrument not None,
        BarType bar_type not None,
        handler not None: Callable[[Bar], None],
    ):
        super().__init__(
            instrument=instrument,
            bar_type=bar_type,
            handler=handler,
        )

        self._cum_value = Decimal(0)  # Cumulative value

    cpdef object get_cumulative_value(self):
        """
        Return the current cumulative value of the aggregator.

        Returns
        -------
        Decimal

        """
        return self._cum_value

    cdef void _apply_update(self, Price price, Quantity size, uint64_t ts_event):
        size_update = size

        while size_update > 0:  # While there is value to apply
            value_update = price * size_update  # Calculated value in quote currency
            if self._cum_value + value_update < self.bar_type.spec.step:
                # Update and break
                self._cum_value = self._cum_value + value_update
                self._builder.update(
                    price=price,
                    size=Quantity(size_update, precision=size._mem.precision),
                    ts_event=ts_event,
                )
                break

            value_diff: Decimal = self.bar_type.spec.step - self._cum_value
            size_diff: Decimal = size_update * (value_diff / value_update)
            # Update builder to the step threshold
            self._builder.update(
                price=price,
                size=Quantity(size_diff, precision=size._mem.precision),
                ts_event=ts_event,
            )

            # Build a bar and reset builder and cumulative value
            self._build_now_and_send()
            self._cum_value = Decimal(0)

            # Decrement the update size
            size_update -= size_diff
            assert size_update >= 0


cdef class TimeBarAggregator(BarAggregator):
    """
    Provides a means of building time bars from ticks with an internal timer.

    When the time reaches the next time interval of the bar specification, then
    a bar is created and sent to the handler.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the aggregator.
    bar_type : BarType
        The bar type for the aggregator.
    handler : Callable[[Bar], None]
        The bar handler for the aggregator.
    clock : Clock
        The clock for the aggregator.
    build_with_no_updates : bool, default True
        If build and emit bars with no new market updates.
    timestamp_on_close : bool, default True
        If timestamp `ts_event` will be bar close.
        If False then timestamp will be bar open.
    interval_type : str, default 'left-open'
        Determines the type of interval used for time aggregation.
        - 'left-open': start time is excluded and end time is included (default).
        - 'right-open': start time is included and end time is excluded.

    Raises
    ------
    ValueError
        If `instrument.id` != `bar_type.instrument_id`.
    """
    def __init__(
        self,
        Instrument instrument not None,
        BarType bar_type not None,
        handler not None: Callable[[Bar], None],
        Clock clock not None,
        bint build_with_no_updates = True,
        bint timestamp_on_close = True,
        str interval_type = "left-open",
    ):
        super().__init__(
            instrument=instrument,
            bar_type=bar_type,
            handler=handler,
        )

        self._clock = clock
        self.interval = self._get_interval()
        self.interval_ns = self._get_interval_ns()
        self._timer_name = None
        self._set_build_timer()
        self.next_close_ns = self._clock.next_time_ns(self._timer_name)
        self._build_on_next_tick = False
        self._stored_open_ns = dt_to_unix_nanos(self.get_start_time())
        self._stored_close_ns = 0
        self._cached_update = None
        self._build_with_no_updates = build_with_no_updates
        self._timestamp_on_close = timestamp_on_close

        if interval_type == "left-open":
            self._is_left_open = True
        elif interval_type == "right-open":
            self._is_left_open = False
        else:
            raise ValueError(
                f"Invalid interval_type: {interval_type}. Must be 'left-open' or 'right-open'.",
            )

    def __str__(self):
        return f"{type(self).__name__}(interval_ns={self.interval_ns}, next_close_ns={self.next_close_ns})"

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
        if self.bar_type.spec.aggregation == BarAggregation.MILLISECOND:
            diff_microseconds = now.microsecond % step // 1000
            diff_seconds = 0 if diff_microseconds == 0 else max(0, (step // 1000) - 1)
            diff = timedelta(
                seconds=diff_seconds,
                microseconds=now.microsecond,
            )
            start_time = now - diff
        elif self.bar_type.spec.aggregation == BarAggregation.SECOND:
            diff_seconds = now.second % step
            diff_minutes = 0 if diff_seconds == 0 else max(0, (step // 60) - 1)
            start_time = now - timedelta(
                minutes=diff_minutes,
                seconds=diff_seconds,
                microseconds=now.microsecond,
            )
        elif self.bar_type.spec.aggregation == BarAggregation.MINUTE:
            diff_minutes = now.minute % step
            diff_hours = 0 if diff_minutes == 0 else max(0, (step // 60) - 1)
            start_time = now - timedelta(
                hours=diff_hours,
                minutes=diff_minutes,
                seconds=now.second,
                microseconds=now.microsecond,
            )
        elif self.bar_type.spec.aggregation == BarAggregation.HOUR:
            diff_hours = now.hour % step
            diff_days = 0 if diff_hours == 0 else max(0, (step // 24) - 1)
            start_time = now - timedelta(
                days=diff_days,
                hours=diff_hours,
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
        else:  # pragma: no cover (design-time error)
            raise ValueError(
                f"Aggregation type not supported for time bars, "
                f"was {bar_aggregation_to_str(self.bar_type.spec.aggregation)}",
            )

        return start_time

    cpdef void stop(self):
        """
        Stop the bar aggregator.
        """
        self._clock.cancel_timer(str(self.bar_type))
        self._timer_name = None

    cdef timedelta _get_interval(self):
        cdef BarAggregation aggregation = self.bar_type.spec.aggregation
        cdef int step = self.bar_type.spec.step

        if aggregation == BarAggregation.MILLISECOND:
            return timedelta(milliseconds=(1 * step))
        elif aggregation == BarAggregation.SECOND:
            return timedelta(seconds=(1 * step))
        elif aggregation == BarAggregation.MINUTE:
            return timedelta(minutes=(1 * step))
        elif aggregation == BarAggregation.HOUR:
            return timedelta(hours=(1 * step))
        elif aggregation == BarAggregation.DAY:
            return timedelta(days=(1 * step))
        else:
            # Design time error
            raise ValueError(
                f"Aggregation not time based, was {bar_aggregation_to_str(aggregation)}",
            )

    cdef uint64_t _get_interval_ns(self):
        cdef BarAggregation aggregation = self.bar_type.spec.aggregation
        cdef int step = self.bar_type.spec.step

        if aggregation == BarAggregation.MILLISECOND:
            return millis_to_nanos(step)
        elif aggregation == BarAggregation.SECOND:
            return secs_to_nanos(step)
        elif aggregation == BarAggregation.MINUTE:
            return secs_to_nanos(step) * 60
        elif aggregation == BarAggregation.HOUR:
            return secs_to_nanos(step) * 60 * 60
        elif aggregation == BarAggregation.DAY:
            return secs_to_nanos(step) * 60 * 60 * 24
        else:
            # Design time error
            raise ValueError(
                f"Aggregation not time based, was {bar_aggregation_to_str(aggregation)}",
            )

    cpdef void _set_build_timer(self):
        self._timer_name = str(self.bar_type)
        self._clock.set_timer(
            name=self._timer_name,
            interval=self.interval,
            start_time=self.get_start_time(),
            stop_time=None,
            callback=self._build_bar,
        )

        self._log.debug(f"Started timer {self._timer_name}.")

    cdef void _apply_update(self, Price price, Quantity size, uint64_t ts_event):
        self._builder.update(price, size, ts_event)
        if self._build_on_next_tick:
            ts_init = ts_event

            # Adjusting the timestamp logic based on interval_type
            if self._is_left_open:
                ts_event = self._stored_close_ns if self._timestamp_on_close else self._stored_open_ns
            else:
                ts_event = self._stored_open_ns

            self._build_and_send(ts_event=ts_event, ts_init=ts_init)
            # Reset flag and clear stored close
            self._build_on_next_tick = False
            self._stored_close_ns = 0

    cpdef void _build_bar(self, TimeEvent event):
        if not self._builder.initialized:
            # Set flag to build on next close with the stored close time
            self._build_on_next_tick = True
            self._stored_close_ns = self.next_close_ns
            return

        if not self._build_with_no_updates and self._builder.count == 0:
            return  # Do not build and emit bar

        cdef uint64_t ts_init = event.ts_event
        cdef uint64_t ts_event
        if self._is_left_open:
            ts_event = event.ts_event if self._timestamp_on_close else self._stored_open_ns
        else:
            ts_event = self._stored_open_ns

        self._build_and_send(ts_event=ts_event, ts_init=ts_init)

        # Close time becomes the next open time
        self._stored_open_ns = event.ts_event

        # On receiving this event, timer should now have a new `next_time_ns`
        self.next_close_ns = self._clock.next_time_ns(self._timer_name)
