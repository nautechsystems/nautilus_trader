# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import pandas as pd

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta
from libc.stdint cimport uint64_t

from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport TimeEvent
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport dt_to_unix_nanos
from nautilus_trader.core.rust.core cimport millis_to_nanos
from nautilus_trader.core.rust.core cimport secs_to_nanos
from nautilus_trader.core.rust.model cimport FIXED_SCALAR
from nautilus_trader.core.rust.model cimport QuantityRaw
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
    ) -> None:
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
            UNIX timestamp (nanoseconds) of the update.

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

    cpdef void update_bar(self, Bar bar, Quantity volume, uint64_t ts_init):
        """
        Update the bar builder.

        Parameters
        ----------
        bar : Bar
            The update Bar.

        """
        Condition.not_none(bar, "bar")

        # TODO: What happens if the first bar updates before a partial bar is applied?
        if ts_init < self.ts_last:
            return  # Not applicable

        if self._open is None:
            # Initialize builder
            self._open = bar.open
            self._high = bar.high
            self._low = bar.low
            self.initialized = True
        else:
            if bar.high > self._high:
                self._high = bar.high

            if bar.low < self._low:
                self._low = bar.low

        self._close = bar.close
        self.volume._mem.raw += volume._mem.raw
        self.count += 1
        self.ts_last = ts_init

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
            UNIX timestamp (nanoseconds) for the bar event.
        ts_init : uint64_t
            UNIX timestamp (nanoseconds) for the bar initialization.

        Returns
        -------
        Bar

        """
        if self._open is None:  # No tick was received
            self._open = self._last_close
            self._high = self._last_close
            self._low = self._last_close
            self._close = self._last_close


        self._low._mem.raw = min(self._close._mem.raw, self._low._mem.raw)
        self._high._mem.raw = max(self._close._mem.raw, self._high._mem.raw)

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
    ) -> None:
        Condition.equal(instrument.id, bar_type.instrument_id, "instrument.id", "bar_type.instrument_id")

        self.bar_type = bar_type
        self._handler = handler
        self._handler_backup = None
        self._await_partial = await_partial
        self._log = Logger(name=type(self).__name__)
        self._builder = BarBuilder(
            instrument=instrument,
            bar_type=self.bar_type,
        )
        self._batch_mode = False
        self.is_running = False # is_running means that an aggregator receives data from the message bus

    def start_batch_update(self, handler: Callable[[Bar], None], uint64_t time_ns) -> None:
        self._batch_mode = True
        self._handler_backup = self._handler
        self._handler = handler
        self._start_batch_time(time_ns)

    def _start_batch_time(self, uint64_t time_ns):
        pass

    def stop_batch_update(self) -> None:
        self._batch_mode = False
        self._handler = self._handler_backup

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
                size=tick.extract_size(self.bar_type.spec.price_type),
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

    cpdef void handle_bar(self, Bar bar):
        """
        Update the aggregator with the given bar.

        Parameters
        ----------
        bar : Bar
            The bar for the update.

        """
        Condition.not_none(bar, "bar")

        if not self._await_partial:
            self._apply_update_bar(
                bar=bar,
                volume=bar.volume,
                ts_init=bar.ts_init,
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
        raise NotImplementedError("method `_apply_update` must be implemented in the subclass")

    cdef void _apply_update_bar(self, Bar bar, Quantity volume, uint64_t ts_init):
        raise NotImplementedError("method `_apply_update` must be implemented in the subclass") # pragma: no cover

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
    ) -> None:
        super().__init__(
            instrument=instrument,
            bar_type=bar_type.standard(),
            handler=handler,
        )

    cdef void _apply_update(self, Price price, Quantity size, uint64_t ts_event):
        self._builder.update(price, size, ts_event)

        if self._builder.count == self.bar_type.spec.step:
            self._build_now_and_send()

    cdef void _apply_update_bar(self, Bar bar, Quantity volume, uint64_t ts_init):
        self._builder.update_bar(bar, volume, ts_init)

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
    ) -> None:
        super().__init__(
            instrument=instrument,
            bar_type=bar_type.standard(),
            handler=handler,
        )

    cdef void _apply_update(self, Price price, Quantity size, uint64_t ts_event):
        cdef QuantityRaw raw_size_update = size._mem.raw
        cdef QuantityRaw raw_step = <QuantityRaw>(self.bar_type.spec.step * <QuantityRaw>FIXED_SCALAR)
        cdef QuantityRaw raw_size_diff = 0

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

    cdef void _apply_update_bar(self, Bar bar, Quantity volume, uint64_t ts_init):
        cdef QuantityRaw raw_volume_update = volume._mem.raw
        cdef QuantityRaw raw_step = <QuantityRaw>(self.bar_type.spec.step * <QuantityRaw>FIXED_SCALAR)
        cdef QuantityRaw raw_volume_diff = 0

        while raw_volume_update > 0:  # While there is volume to apply
            if self._builder.volume._mem.raw + raw_volume_update < raw_step:
                # Update and break
                self._builder.update_bar(
                    bar=bar,
                    volume=Quantity.from_raw_c(raw_volume_update, precision=volume._mem.precision),
                    ts_init=ts_init,
                )
                break

            raw_volume_diff = raw_step - self._builder.volume._mem.raw
            # Update builder to the step threshold
            self._builder.update_bar(
                bar=bar,
                volume=Quantity.from_raw_c(raw_volume_diff, precision=volume._mem.precision),
                ts_init=ts_init,
            )

            # Build a bar and reset builder
            self._build_now_and_send()

            # Decrement the update volume
            raw_volume_update -= raw_volume_diff
            assert raw_volume_update >= 0


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
    ) -> None:
        super().__init__(
            instrument=instrument,
            bar_type=bar_type.standard(),
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

    cdef void _apply_update_bar(self, Bar bar, Quantity volume, uint64_t ts_init):
        volume_update = volume
        average_price = Quantity((bar.high + bar.low + bar.close) / Decimal(3.0),
                                 precision=self._builder.price_precision)

        while volume_update > 0:  # While there is value to apply
            value_update = average_price * volume_update  # Calculated value in quote currency
            if self._cum_value + value_update < self.bar_type.spec.step:
                # Update and break
                self._cum_value = self._cum_value + value_update
                self._builder.update_bar(
                    bar=bar,
                    volume=Quantity(volume_update, precision=volume._mem.precision),
                    ts_init=ts_init,
                )
                break

            value_diff: Decimal = self.bar_type.spec.step - self._cum_value
            volume_diff: Decimal = volume_update * (value_diff / value_update)
            # Update builder to the step threshold
            self._builder.update_bar(
                bar=bar,
                volume=Quantity(volume_diff, precision=volume._mem.precision),
                ts_init=ts_init,
            )

            # Build a bar and reset builder and cumulative value
            self._build_now_and_send()
            self._cum_value = Decimal(0)

            # Decrement the update volume
            volume_update -= volume_diff
            assert volume_update >= 0


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
    interval_type : str, default 'left-open'
        Determines the type of interval used for time aggregation.
        - 'left-open': start time is excluded and end time is included (default).
        - 'right-open': start time is included and end time is excluded.
    timestamp_on_close : bool, default True
        If True, then timestamp will be the bar close time.
        If False, then timestamp will be the bar open time.
    skip_first_non_full_bar : bool, default False
        If will skip emitting a bar if the aggregation starts mid-interval.
    build_with_no_updates : bool, default True
        If build and emit bars with no new market updates.
    time_bars_origin : pd.Timedelta or pd.DateOffset, optional
        The origin time offset.
    composite_bar_build_delay : int, default 15
        The time delay (microseconds) before building and emitting a composite bar type.

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
        str interval_type = "left-open",
        bint timestamp_on_close = True,
        bint skip_first_non_full_bar = False,
        bint build_with_no_updates = True,
        object time_bars_origin: pd.Timedelta | pd.DateOffset = None,
        int composite_bar_build_delay = 15, # in microsecond
    ) -> None:
        super().__init__(
            instrument=instrument,
            bar_type=bar_type.standard(),
            handler=handler,
        )

        self._clock = clock
        self.interval = self._get_interval()
        self.interval_ns = self._get_interval_ns()
        self._timer_name = None
        self._set_build_timer()
        self.next_close_ns = self._clock.next_time_ns(self._timer_name)
        self._build_on_next_tick = False
        cdef datetime now = self._clock.utc_now()
        self._stored_open_ns = dt_to_unix_nanos(self.get_start_time(now))
        self._stored_close_ns = 0
        self._cached_update = None
        self._build_with_no_updates = build_with_no_updates
        self._timestamp_on_close = timestamp_on_close
        self._composite_bar_build_delay = composite_bar_build_delay
        self._add_delay = bar_type.is_composite() and bar_type.composite().is_internally_aggregated()
        self._batch_open_ns = 0
        self._batch_next_close_ns = 0
        self._time_bars_origin = time_bars_origin
        self._skip_first_non_full_bar = skip_first_non_full_bar

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

    def get_start_time(self, now: datetime) -> datetime:
        """
        Return the start time for the aggregators next bar.

        Returns
        -------
        datetime
            The timestamp (UTC).

        """
        step = self.bar_type.spec.step
        aggregation = self.bar_type.spec.aggregation

        if aggregation == BarAggregation.MILLISECOND:
            start_time = now.floor(freq="s")

            if self._time_bars_origin is not None:
                start_time += self._time_bars_origin

            if now < start_time:
                start_time -= pd.Timedelta(seconds=1)

            while start_time <= now:
                start_time += pd.Timedelta(milliseconds=step)

            start_time -= pd.Timedelta(milliseconds=step)
        elif aggregation == BarAggregation.SECOND:
            start_time = now.floor(freq="min")

            if self._time_bars_origin is not None:
                start_time += self._time_bars_origin

            if now < start_time:
                start_time -= pd.Timedelta(minutes=1)

            while start_time <= now:
                start_time += pd.Timedelta(seconds=step)

            start_time -= pd.Timedelta(seconds=step)
        elif aggregation == BarAggregation.MINUTE:
            start_time = now.floor(freq="h")

            if self._time_bars_origin is not None:
                start_time += self._time_bars_origin

            if now < start_time:
                start_time -= pd.Timedelta(hours=1)

            while start_time <= now:
                start_time += pd.Timedelta(minutes=step)

            start_time -= pd.Timedelta(minutes=step)
        elif aggregation == BarAggregation.HOUR:
            start_time = now.floor(freq="d")

            if self._time_bars_origin is not None:
                start_time += self._time_bars_origin

            if now < start_time:
                start_time -= pd.Timedelta(days=1)

            while start_time <= now:
                start_time += pd.Timedelta(hours=step)

            start_time -= pd.Timedelta(hours=step)
        elif aggregation == BarAggregation.DAY:
            start_time = now.floor(freq="d")

            if self._time_bars_origin is not None:
                start_time += self._time_bars_origin

            if now < start_time:
                start_time -= pd.Timedelta(days=1)
        elif aggregation == BarAggregation.WEEK:
            start_time = (now - pd.Timedelta(days=now.dayofweek)).floor(freq="d")

            if self._time_bars_origin is not None:
                start_time += self._time_bars_origin

            if now < start_time:
                start_time -= pd.Timedelta(weeks=1)
        elif aggregation == BarAggregation.MONTH:
            start_time = (now - pd.DateOffset(months=now.month - 1, days=now.day - 1)).floor(freq="d")

            if self._time_bars_origin is not None:
                start_time += self._time_bars_origin

            if now < start_time:
                start_time -= pd.DateOffset(years=1)

            while start_time <= now:
                start_time += pd.DateOffset(months=step)

            start_time -= pd.DateOffset(months=step)
        else:  # pragma: no cover (design-time error)
            raise ValueError(
                f"Aggregation type not supported for time bars, "
                f"was {bar_aggregation_to_str(aggregation)}",
            )

        return start_time

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
        elif aggregation == BarAggregation.WEEK:
            return timedelta(days=(7 * step))
        elif aggregation == BarAggregation.MONTH:
            # not actually used
            return timedelta(days=0)
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
        elif aggregation == BarAggregation.WEEK:
            return secs_to_nanos(step) * 60 * 60 * 24 * 7
        elif aggregation == BarAggregation.MONTH:
            # not actually used
            return 0
        else:
            # Design time error
            raise ValueError(
                f"Aggregation not time based, was {bar_aggregation_to_str(aggregation)}",
            )

    cpdef void _set_build_timer(self):
        cdef int step = self.bar_type.spec.step
        self._timer_name = str(self.bar_type)
        cdef datetime now = self._clock.utc_now()
        cdef datetime start_time = self.get_start_time(now)

        if start_time == now:
            self._skip_first_non_full_bar = False

        if self._add_delay:
            start_time += timedelta(microseconds=self._composite_bar_build_delay)

        if self.bar_type.spec.aggregation != BarAggregation.MONTH:
            self._clock.set_timer(
                name=self._timer_name,
                interval=self.interval,
                start_time=start_time,
                stop_time=None,
                callback=self._build_bar,
            )
        else:
            # The monthly alert time is defined iteratively at each alert time as there is no regular interval
            alert_time = start_time + pd.DateOffset(months=step)

            self._clock.set_time_alert(
                name=self._timer_name,
                alert_time=alert_time,
                callback=self._build_bar,
                override=True,
            )

        self._log.debug(f"Started timer {self._timer_name}")

    cpdef void stop(self):
        """
        Stop the bar aggregator.
        """
        self._clock.cancel_timer(str(self.bar_type))

    cdef void _build_and_send(self, uint64_t ts_event, uint64_t ts_init):
        if self._skip_first_non_full_bar:
            self._builder.reset()
            self._skip_first_non_full_bar = False
        else:
            BarAggregator._build_and_send(self, ts_event, ts_init)

    def _start_batch_time(self, uint64_t time_ns):
        cdef int step = self.bar_type.spec.step
        self._batch_mode = True

        start_time = self.get_start_time(unix_nanos_to_dt(time_ns))
        self._batch_open_ns = dt_to_unix_nanos(start_time)

        if self.bar_type.spec.aggregation != BarAggregation.MONTH:
            if self._batch_open_ns == time_ns:
                self._batch_open_ns -= self.interval_ns

            self._batch_next_close_ns = self._batch_open_ns + self.interval_ns
        else:
            if self._batch_open_ns == time_ns:
                self._batch_open_ns = dt_to_unix_nanos(unix_nanos_to_dt(self._batch_open_ns) - pd.DateOffset(months=step))

            self._batch_next_close_ns = dt_to_unix_nanos(unix_nanos_to_dt(self._batch_open_ns) + pd.DateOffset(months=step))

    cdef void _batch_pre_update(self, uint64_t time_ns):
        if time_ns > self._batch_next_close_ns and self._builder.initialized:
            ts_init = self._batch_next_close_ns

            # Adjusting the timestamp logic based on interval_type
            if self._is_left_open:
                ts_event = self._batch_next_close_ns if self._timestamp_on_close else self._batch_open_ns
            else:
                ts_event = self._batch_open_ns

            self._build_and_send(ts_event=ts_event, ts_init=ts_init)

    cdef void _batch_post_update(self, uint64_t time_ns):
        cdef int step = self.bar_type.spec.step

        # Update has already been done, resetting _batch_next_close_ns
        if not self._batch_mode and time_ns == self._batch_next_close_ns and time_ns > self._stored_open_ns:
            self._batch_next_close_ns = 0
            return

        if time_ns > self._batch_next_close_ns:
            # We ensure that _batch_next_close_ns and _batch_open_ns are coherent with the last builder update
            if self.bar_type.spec.aggregation != BarAggregation.MONTH:
                while self._batch_next_close_ns < time_ns:
                    self._batch_next_close_ns += self.interval_ns

                self._batch_open_ns = self._batch_next_close_ns - self.interval_ns
            else:
                while self._batch_next_close_ns < time_ns:
                    self._batch_next_close_ns = dt_to_unix_nanos(unix_nanos_to_dt(self._batch_next_close_ns) + pd.DateOffset(months=step))

                self._batch_open_ns = dt_to_unix_nanos(unix_nanos_to_dt(self._batch_next_close_ns) - pd.DateOffset(months=step))

        if time_ns == self._batch_next_close_ns:
            # Adjusting the timestamp logic based on interval_type
            if self._is_left_open:
                ts_event = self._batch_next_close_ns if self._timestamp_on_close else self._batch_open_ns
            else:
                ts_event = self._batch_open_ns

            self._build_and_send(ts_event=ts_event, ts_init=time_ns)
            self._batch_open_ns = self._batch_next_close_ns

            if self.bar_type.spec.aggregation != BarAggregation.MONTH:
                self._batch_next_close_ns += self.interval_ns
            else:
                self._batch_next_close_ns = dt_to_unix_nanos(unix_nanos_to_dt(self._batch_next_close_ns) + pd.DateOffset(months=step))

        # Delay to reset of _batch_next_close_ns to allow the creation of a last histo bar
        # when transitioning to regular bars
        if not self._batch_mode:
            self._batch_next_close_ns = 0

    cdef void _apply_update(self, Price price, Quantity size, uint64_t ts_event):
        if self._batch_next_close_ns != 0:
            self._batch_pre_update(ts_event)

        self._builder.update(price, size, ts_event)

        if self._build_on_next_tick:
            if ts_event <= self._stored_close_ns:
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

        if self._batch_next_close_ns != 0:
            self._batch_post_update(ts_event)

    cdef void _apply_update_bar(self, Bar bar, Quantity volume, uint64_t ts_init):
        if self._batch_next_close_ns != 0:
            self._batch_pre_update(ts_init)

        self._builder.update_bar(bar, volume, ts_init)

        if self._build_on_next_tick:
            if ts_init <= self._stored_close_ns:
                # Adjusting the timestamp logic based on interval_type
                if self._is_left_open:
                    ts_event = self._stored_close_ns if self._timestamp_on_close else self._stored_open_ns
                else:
                    ts_event = self._stored_open_ns

                self._build_and_send(ts_event=ts_event, ts_init=ts_init)

            # Reset flag and clear stored close
            self._build_on_next_tick = False
            self._stored_close_ns = 0

        if self._batch_next_close_ns != 0:
            self._batch_post_update(ts_init)

    cpdef void _build_bar(self, TimeEvent event):
        if not self._builder.initialized:
            # Set flag to build on next close with the stored close time
            # _build_on_next_tick is used to avoid a race condition between a data update and a TimeEvent from the timer
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

        cdef int step = self.bar_type.spec.step

        if self.bar_type.spec.aggregation != BarAggregation.MONTH:
            # On receiving this event, timer should now have a new `next_time_ns`
            self.next_close_ns = self._clock.next_time_ns(self._timer_name)
        else:
            alert_time = unix_nanos_to_dt(event.ts_event) + pd.DateOffset(months=step)

            self._clock.set_time_alert(
                name=self._timer_name,
                alert_time=alert_time,
                callback=self._build_bar,
                override=True,
            )

            self.next_close_ns = dt_to_unix_nanos(alert_time)
