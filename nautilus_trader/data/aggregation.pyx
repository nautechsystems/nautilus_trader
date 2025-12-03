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

import numpy as np
import pandas as pd

cimport numpy as np
from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta
from libc.stdint cimport uint64_t

from datetime import timedelta

from nautilus_trader.core.datetime import unix_nanos_to_dt

from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport TimeEvent
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport dt_to_unix_nanos
from nautilus_trader.core.rust.core cimport millis_to_nanos
from nautilus_trader.core.rust.core cimport secs_to_nanos
from nautilus_trader.core.rust.model cimport FIXED_SCALAR
from nautilus_trader.core.rust.model cimport AggressorSide
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

    cpdef void update(self, Price price, Quantity size, uint64_t ts_init):
        """
        Update the bar builder.

        Parameters
        ----------
        price : Price
            The update price.
        size : Decimal
            The update size.
        ts_init : uint64_t
            UNIX timestamp (nanoseconds) of the update.

        """
        Condition.not_none(price, "price")
        Condition.not_none(size, "size")

        if ts_init < self.ts_last:
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
        self.ts_last = ts_init

    cpdef void update_bar(self, Bar bar, Quantity volume, uint64_t ts_init):
        """
        Update the bar builder.

        Parameters
        ----------
        bar : Bar
            The update Bar.

        """
        Condition.not_none(bar, "bar")

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

    The aggregator maintains two state flags exposed as properties:
    - `historical_mode`: Indicates the aggregator is processing historical data.
    - `is_running`: Indicates the aggregator is receiving data from the message bus.

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
        Condition.equal(instrument.id, bar_type.instrument_id, "instrument.id", "bar_type.instrument_id")

        self._handler = handler
        self._handler_backup = None
        self._log = Logger(name=type(self).__name__)
        self._builder = BarBuilder(
            instrument=instrument,
            bar_type=bar_type,
        )

        self.bar_type = bar_type
        self.historical_mode = False
        self.is_running = False

    cpdef void set_historical_mode(self, bint historical_mode, handler: Callable[[Bar], None]):
        """
        Set the historical mode state of the aggregator.

        Parameters
        ----------
        historical_mode : bool
            Whether the aggregator is processing historical data.
        handler : Callable[[Bar], None]
            The bar handler to use in this mode.

        Raises
        ------
        TypeError
            If `handler` is ``None`` or not callable.

        """
        Condition.callable(handler, "handler")

        self.historical_mode = historical_mode
        self._handler = handler

    cpdef void set_running(self, bint is_running):
        """
        Set the running state of the aggregator.

        Parameters
        ----------
        is_running : bool
            Whether the aggregator is running (receiving data from message bus).

        """
        self.is_running = is_running

    cpdef void handle_quote_tick(self, QuoteTick tick):
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
            size=tick.extract_size(self.bar_type.spec.price_type),
            ts_init=tick.ts_init,
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

        self._apply_update(
            price=tick.price,
            size=tick.size,
            ts_init=tick.ts_init,
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

        self._apply_update_bar(
            bar=bar,
            volume=bar.volume,
            ts_init=bar.ts_init,
        )

    cdef void _apply_update(self, Price price, Quantity size, uint64_t ts_init):
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
            bar_type=bar_type,
            handler=handler,
        )

    cdef void _apply_update(self, Price price, Quantity size, uint64_t ts_init):
        self._builder.update(price, size, ts_init)

        if self._builder.count == self.bar_type.spec.step:
            self._build_now_and_send()

    cdef void _apply_update_bar(self, Bar bar, Quantity volume, uint64_t ts_init):
        self._builder.update_bar(bar, volume, ts_init)

        if self._builder.count == self.bar_type.spec.step:
            self._build_now_and_send()


cdef class TickImbalanceBarAggregator(BarAggregator):
    """
    Provides a means of building tick imbalance bars from ticks.

    When the absolute difference between buy and sell ticks reaches the step
    threshold of the bar specification, then a bar is created and sent to the
    handler.

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
            bar_type=bar_type,
            handler=handler,
        )
        self._imbalance = 0

    cdef void _apply_update(self, Price price, Quantity size, uint64_t ts_init):
        self._builder.update(price, size, ts_init)

    cdef void _apply_update_bar(self, Bar bar, Quantity volume, uint64_t ts_init):
        self._builder.update_bar(bar, volume, ts_init)

    cpdef void handle_trade_tick(self, TradeTick tick):
        Condition.not_none(tick, "tick")

        cdef AggressorSide side = tick.aggressor_side
        if side == AggressorSide.NO_AGGRESSOR:
            self._apply_update(tick.price, tick.size, tick.ts_init)
            return

        self._apply_update(tick.price, tick.size, tick.ts_init)

        if side == AggressorSide.BUYER:
            self._imbalance += 1
        else:
            self._imbalance -= 1

        if abs(self._imbalance) >= self.bar_type.spec.step:
            self._build_now_and_send()
            self._imbalance = 0


cdef class TickRunsBarAggregator(BarAggregator):
    """
    Provides a means of building tick runs bars from ticks.

    When consecutive ticks of the same aggressor side reach the step threshold
    of the bar specification, then a bar is created and sent to the handler.
    The run resets when the aggressor side changes.

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
            bar_type=bar_type,
            handler=handler,
        )

        self._current_run_side = AggressorSide.NO_AGGRESSOR
        self._has_run_side = False
        self._run_count = 0

    cdef void _apply_update(self, Price price, Quantity size, uint64_t ts_init):
        self._builder.update(price, size, ts_init)

    cdef void _apply_update_bar(self, Bar bar, Quantity volume, uint64_t ts_init):
        self._builder.update_bar(bar, volume, ts_init)

    cpdef void handle_trade_tick(self, TradeTick tick):
        Condition.not_none(tick, "tick")

        cdef AggressorSide side = tick.aggressor_side
        if side == AggressorSide.NO_AGGRESSOR:
            self._apply_update(tick.price, tick.size, tick.ts_init)
            return

        if not self._has_run_side or self._current_run_side != side:
            self._current_run_side = side
            self._has_run_side = True
            self._run_count = 0
            self._builder.reset()

        self._apply_update(tick.price, tick.size, tick.ts_init)
        self._run_count += 1

        if self._run_count >= self.bar_type.spec.step:
            self._build_now_and_send()
            self._run_count = 0
            self._has_run_side = False


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
            bar_type=bar_type,
            handler=handler,
        )

    cdef void _apply_update(self, Price price, Quantity size, uint64_t ts_init):
        cdef QuantityRaw raw_size_update = size._mem.raw
        cdef QuantityRaw raw_step = <QuantityRaw>(self.bar_type.spec.step * <QuantityRaw>FIXED_SCALAR)
        cdef QuantityRaw raw_size_diff = 0

        while raw_size_update > 0:  # While there is size to apply
            if self._builder.volume._mem.raw + raw_size_update < raw_step:
                # Update and break
                self._builder.update(
                    price=price,
                    size=Quantity.from_raw_c(raw_size_update, precision=size._mem.precision),
                    ts_init=ts_init,
                )
                break

            raw_size_diff = raw_step - self._builder.volume._mem.raw
            # Update builder to the step threshold
            self._builder.update(
                price=price,
                size=Quantity.from_raw_c(raw_size_diff, precision=size._mem.precision),
                ts_init=ts_init,
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


cdef class VolumeImbalanceBarAggregator(BarAggregator):
    """
    Provides a means of building volume imbalance bars from ticks.

    When the absolute difference between buy and sell volume reaches the step
    threshold of the bar specification, then a bar is created and sent to the
    handler.

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
            bar_type=bar_type,
            handler=handler,
        )
        cdef long long step_value = self.bar_type.spec.step
        self._imbalance_raw = 0
        self._raw_step = <long long>(step_value * FIXED_SCALAR)

    cdef void _apply_update(self, Price price, Quantity size, uint64_t ts_init):
        self._builder.update(price, size, ts_init)

    cdef void _apply_update_bar(self, Bar bar, Quantity volume, uint64_t ts_init):
        self._builder.update_bar(bar, volume, ts_init)

    cpdef void handle_trade_tick(self, TradeTick tick):
        Condition.not_none(tick, "tick")

        cdef AggressorSide side = tick.aggressor_side
        if side == AggressorSide.NO_AGGRESSOR:
            self._apply_update(tick.price, tick.size, tick.ts_init)
            return

        cdef long long side_sign = 1 if side == AggressorSide.BUYER else -1
        cdef double size_remaining = float(tick.size)
        cdef double size_chunk
        cdef double needed_qty
        cdef long long imbalance_abs
        cdef long long needed

        while size_remaining > 0.0:
            imbalance_abs = abs(self._imbalance_raw)
            needed = self._raw_step - imbalance_abs
            if needed <= 0:
                needed = 1

            # Convert needed from raw (10^9 scale) to quantity
            needed_qty = <double>needed / <double>FIXED_SCALAR
            if size_remaining <= needed_qty:
                self._imbalance_raw += side_sign * <long long>(size_remaining * FIXED_SCALAR)
                self._apply_update(
                    tick.price,
                    Quantity(size_remaining, precision=tick.size.precision),
                    tick.ts_init,
                )

                if abs(self._imbalance_raw) >= self._raw_step:
                    self._build_now_and_send()
                    self._imbalance_raw = 0
                break

            size_chunk = needed_qty
            self._apply_update(
                tick.price,
                Quantity(size_chunk, precision=tick.size.precision),
                tick.ts_init,
            )
            self._imbalance_raw += side_sign * needed
            size_remaining -= size_chunk

            if abs(self._imbalance_raw) >= self._raw_step:
                self._build_now_and_send()
                self._imbalance_raw = 0


cdef class VolumeRunsBarAggregator(BarAggregator):
    """
    Provides a means of building volume runs bars from ticks.

    When consecutive volume of the same aggressor side reaches the step
    threshold of the bar specification, then a bar is created and sent to the
    handler. The run resets when the aggressor side changes.

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
            bar_type=bar_type,
            handler=handler,
        )
        cdef long long step_value = self.bar_type.spec.step
        self._current_run_side = AggressorSide.NO_AGGRESSOR
        self._has_run_side = False
        self._run_volume_raw = 0
        self._raw_step = <long long>(step_value * FIXED_SCALAR)

    cdef void _apply_update(self, Price price, Quantity size, uint64_t ts_init):
        self._builder.update(price, size, ts_init)

    cdef void _apply_update_bar(self, Bar bar, Quantity volume, uint64_t ts_init):
        self._builder.update_bar(bar, volume, ts_init)

    cpdef void handle_trade_tick(self, TradeTick tick):
        Condition.not_none(tick, "tick")

        cdef AggressorSide side = tick.aggressor_side
        if side == AggressorSide.NO_AGGRESSOR:
            self._apply_update(tick.price, tick.size, tick.ts_init)
            return

        if not self._has_run_side or self._current_run_side != side:
            self._current_run_side = side
            self._has_run_side = True
            self._run_volume_raw = 0
            self._builder.reset()

        cdef double size_remaining = float(tick.size)
        cdef double size_chunk
        cdef double needed_qty
        cdef long long needed

        while size_remaining > 0.0:
            needed = self._raw_step - self._run_volume_raw
            if needed <= 0:
                needed = 1

            # Convert needed from raw (10^9 scale) to quantity
            needed_qty = <double>needed / <double>FIXED_SCALAR
            if size_remaining <= needed_qty:
                self._run_volume_raw += <long long>(size_remaining * FIXED_SCALAR)
                self._apply_update(
                    tick.price,
                    Quantity(size_remaining, precision=tick.size.precision),
                    tick.ts_init,
                )

                if self._run_volume_raw >= self._raw_step:
                    self._build_now_and_send()
                    self._run_volume_raw = 0
                    self._has_run_side = False
                break

            size_chunk = needed_qty
            self._apply_update(
                tick.price,
                Quantity(size_chunk, precision=tick.size.precision),
                tick.ts_init,
            )
            self._run_volume_raw += needed
            size_remaining -= size_chunk

            if self._run_volume_raw >= self._raw_step:
                self._build_now_and_send()
                self._run_volume_raw = 0
                self._has_run_side = False


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

    cdef void _apply_update(self, Price price, Quantity size, uint64_t ts_init):
        size_update = size

        while size_update > 0:  # While there is value to apply
            value_update = price * size_update  # Calculated value in quote currency
            if self._cum_value + value_update < self.bar_type.spec.step:
                # Update and break
                self._cum_value = self._cum_value + value_update
                self._builder.update(
                    price=price,
                    size=Quantity(size_update, precision=size._mem.precision),
                    ts_init=ts_init,
                )
                break

            value_diff: Decimal = self.bar_type.spec.step - self._cum_value
            size_diff: Decimal = size_update * (value_diff / value_update)
            # Update builder to the step threshold
            self._builder.update(
                price=price,
                size=Quantity(size_diff, precision=size._mem.precision),
                ts_init=ts_init,
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


cdef class ValueImbalanceBarAggregator(BarAggregator):
    """
    Provides a means of building value imbalance bars from ticks.

    When the absolute difference between buy and sell notional value reaches
    the step threshold of the bar specification, then a bar is created and
    sent to the handler.

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
            bar_type=bar_type,
            handler=handler,
        )
        self._imbalance_value = 0.0
        self._step_value = float(self.bar_type.spec.step)

    cdef void _apply_update(self, Price price, Quantity size, uint64_t ts_init):
        self._builder.update(price, size, ts_init)

    cdef void _apply_update_bar(self, Bar bar, Quantity volume, uint64_t ts_init):
        self._builder.update_bar(bar, volume, ts_init)

    cpdef void handle_trade_tick(self, TradeTick tick):
        Condition.not_none(tick, "tick")

        cdef double price_f64 = float(tick.price)
        if price_f64 == 0.0:
            self._apply_update(tick.price, tick.size, tick.ts_init)
            return

        cdef AggressorSide side = tick.aggressor_side
        if side == AggressorSide.NO_AGGRESSOR:
            self._apply_update(tick.price, tick.size, tick.ts_init)
            return

        cdef double side_sign = 1.0 if side == AggressorSide.BUYER else -1.0
        cdef double size_remaining = float(tick.size)
        cdef double value_remaining
        cdef double current_sign
        cdef double needed
        cdef double value_chunk
        cdef double size_chunk
        cdef double imbalance_abs
        cdef double value_to_flatten

        while size_remaining > 0.0:
            value_remaining = price_f64 * size_remaining
            current_sign = 0.0
            if self._imbalance_value > 0.0:
                current_sign = 1.0
            elif self._imbalance_value < 0.0:
                current_sign = -1.0

            if current_sign == 0.0 or current_sign == side_sign:
                needed = self._step_value - abs(self._imbalance_value)
                if value_remaining <= needed:
                    self._imbalance_value += side_sign * value_remaining
                    self._apply_update(
                        tick.price,
                        Quantity(size_remaining, precision=tick.size.precision),
                        tick.ts_init,
                    )

                    if abs(self._imbalance_value) >= self._step_value:
                        self._build_now_and_send()
                        self._imbalance_value = 0.0
                    break

                value_chunk = needed
                size_chunk = value_chunk / price_f64
                self._apply_update(
                    tick.price,
                    Quantity(size_chunk, precision=tick.size.precision),
                    tick.ts_init,
                )
                self._imbalance_value += side_sign * value_chunk
                size_remaining -= size_chunk

                if abs(self._imbalance_value) >= self._step_value:
                    self._build_now_and_send()
                    self._imbalance_value = 0.0
            else:
                imbalance_abs = abs(self._imbalance_value)
                value_to_flatten = value_remaining if value_remaining < imbalance_abs else imbalance_abs
                size_chunk = value_to_flatten / price_f64
                self._apply_update(
                    tick.price,
                    Quantity(size_chunk, precision=tick.size.precision),
                    tick.ts_init,
                )
                self._imbalance_value += side_sign * value_to_flatten
                size_remaining -= size_chunk


cdef class ValueRunsBarAggregator(BarAggregator):
    """
    Provides a means of building value runs bars from ticks.

    When consecutive notional value of the same aggressor side reaches the
    step threshold of the bar specification, then a bar is created and sent
    to the handler. The run resets when the aggressor side changes.

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
            bar_type=bar_type,
            handler=handler,
        )
        self._current_run_side = AggressorSide.NO_AGGRESSOR
        self._has_run_side = False
        self._run_value = 0.0
        self._step_value = float(self.bar_type.spec.step)

    cdef void _apply_update(self, Price price, Quantity size, uint64_t ts_init):
        self._builder.update(price, size, ts_init)

    cdef void _apply_update_bar(self, Bar bar, Quantity volume, uint64_t ts_init):
        self._builder.update_bar(bar, volume, ts_init)

    cpdef void handle_trade_tick(self, TradeTick tick):
        Condition.not_none(tick, "tick")

        cdef double price_f64 = float(tick.price)
        if price_f64 == 0.0:
            self._apply_update(tick.price, tick.size, tick.ts_init)
            return

        cdef AggressorSide side = tick.aggressor_side
        if side == AggressorSide.NO_AGGRESSOR:
            self._apply_update(tick.price, tick.size, tick.ts_init)
            return

        if not self._has_run_side or self._current_run_side != side:
            self._current_run_side = side
            self._has_run_side = True
            self._run_value = 0.0
            self._builder.reset()

        cdef double size_remaining = float(tick.size)
        cdef double value_update
        cdef double value_needed
        cdef double size_chunk

        while size_remaining > 0.0:
            value_update = price_f64 * size_remaining
            if self._run_value + value_update < self._step_value:
                self._run_value += value_update
                self._apply_update(
                    tick.price,
                    Quantity(size_remaining, precision=tick.size.precision),
                    tick.ts_init,
                )

                if self._run_value >= self._step_value:
                    self._build_now_and_send()
                    self._run_value = 0.0
                    self._has_run_side = False
                break

            value_needed = self._step_value - self._run_value
            size_chunk = value_needed / price_f64
            self._apply_update(
                tick.price,
                Quantity(size_chunk, precision=tick.size.precision),
                tick.ts_init,
            )

            self._build_now_and_send()
            self._run_value = 0.0
            self._has_run_side = False
            size_remaining -= size_chunk


cdef class RenkoBarAggregator(BarAggregator):
    """
    Provides a means of building Renko bars from ticks.

    Renko bars are created when the price moves by a fixed amount (brick size)
    regardless of time or volume. Each bar represents a price movement equal
    to the step size in the bar specification.

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
            bar_type=bar_type,
            handler=handler,
        )

        # Calculate brick size from step and instrument price increment
        # step represents number of ticks, so brick_size = step * price_increment
        self.brick_size = instrument.price_increment.as_decimal() * Decimal(bar_type.spec.step)
        self._last_close = None

    cdef void _apply_update(self, Price price, Quantity size, uint64_t ts_init):
        # Initialize last_close if this is the first update
        if self._last_close is None:
            self._last_close = price
            # For the first update, just store the price and add to builder
            self._builder.update(price, size, ts_init)
            return

        # Always update the builder with the current tick
        self._builder.update(price, size, ts_init)

        last_close = self._last_close
        price_diff_decimal = price.as_decimal() - last_close.as_decimal()
        abs_price_diff = abs(price_diff_decimal)

        # Check if we need to create one or more Renko bars
        if abs_price_diff >= self.brick_size:
            num_bricks = int(abs_price_diff // self.brick_size)
            direction = 1 if price_diff_decimal > 0 else -1
            current_close = last_close

            # Store the current builder volume to distribute across bricks
            total_volume = self._builder.volume

            for i in range(num_bricks):
                # Calculate the close price for this brick
                brick_close_decimal = current_close.as_decimal() + (direction * self.brick_size)
                brick_close = Price.from_str(str(brick_close_decimal))

                # Set the builder's OHLC for this specific brick
                self._builder._open = current_close
                self._builder._close = brick_close

                if direction > 0:
                    # Upward movement: high = brick_close, low = current_close
                    self._builder._high = brick_close
                    self._builder._low = current_close
                else:
                    # Downward movement: high = current_close, low = brick_close
                    self._builder._high = current_close
                    self._builder._low = brick_close

                # Set the volume for this brick (all accumulated volume goes to each brick)
                self._builder.volume = total_volume

                # Build and send the bar
                self._build_and_send(ts_init, ts_init)

                # Update current_close for the next brick
                current_close = brick_close

            # Update last_close to the final brick close
            self._last_close = current_close
        # If price movement is less than brick size, we accumulate in the builder

    cdef void _apply_update_bar(self, Bar bar, Quantity volume, uint64_t ts_init):
        # Initialize last_close if this is the first update
        if self._last_close is None:
            self._last_close = bar.close
            # For the first update, just store the price and add to builder
            self._builder.update_bar(bar, volume, ts_init)
            return

        # Always update the builder with the current bar
        self._builder.update_bar(bar, volume, ts_init)

        last_close = self._last_close
        price_diff_decimal = bar.close.as_decimal() - last_close.as_decimal()
        abs_price_diff = abs(price_diff_decimal)

        # Check if we need to create one or more Renko bars
        if abs_price_diff >= self.brick_size:
            num_bricks = int(abs_price_diff // self.brick_size)
            direction = 1 if price_diff_decimal > 0 else -1
            current_close = last_close

            # Store the current builder volume to distribute across bricks
            total_volume = self._builder.volume

            for i in range(num_bricks):
                # Calculate the close price for this brick
                brick_close_decimal = current_close.as_decimal() + (direction * self.brick_size)
                brick_close = Price.from_str(str(brick_close_decimal))

                # Set the builder's OHLC for this specific brick
                self._builder._open = current_close
                self._builder._close = brick_close

                if direction > 0:
                    # Upward movement: high = brick_close, low = current_close
                    self._builder._high = brick_close
                    self._builder._low = current_close
                else:
                    # Downward movement: high = current_close, low = brick_close
                    self._builder._high = current_close
                    self._builder._low = brick_close

                # Set the volume for this brick (all accumulated volume goes to each brick)
                self._builder.volume = total_volume

                # Build and send the bar
                self._build_and_send(ts_init, ts_init)

                # Update current_close for the next brick
                current_close = brick_close

            # Update last_close to the final brick close
            self._last_close = current_close
        # If price movement is less than brick size, we accumulate in the builder


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
    time_bars_origin_offset : pd.Timedelta or pd.DateOffset, optional
        The origin time offset.
    bar_build_delay : int, default 0
        The time delay (microseconds) before building and emitting a composite bar type.
        15 microseconds can be useful in a backtest context, when aggregating internal bars
        from internal bars several times so all messages are processed before a timer triggers.

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
        object time_bars_origin_offset: pd.Timedelta | pd.DateOffset = None,
        int bar_build_delay = 0,
    ) -> None:
        super().__init__(
            instrument=instrument,
            bar_type=bar_type,
            handler=handler,
        )
        self._clock = clock
        self._timestamp_on_close = timestamp_on_close
        self._skip_first_non_full_bar = skip_first_non_full_bar
        self._build_with_no_updates = build_with_no_updates
        self._bar_build_delay = bar_build_delay
        self._time_bars_origin_offset = time_bars_origin_offset or 0
        self._timer_name = str(self.bar_type)
        self.interval = self._get_interval()
        self.interval_ns = self._get_interval_ns()
        self.stored_open_ns = 0
        self.next_close_ns = 0
        self.historical_mode = False
        self._historical_events = []

        if interval_type == "left-open":
            self._is_left_open = True
        elif interval_type == "right-open":
            self._is_left_open = False
        else:
            raise ValueError(
                f"Invalid interval_type: {interval_type}. Must be 'left-open' or 'right-open'.",
            )

        if type(self._time_bars_origin_offset) is int:
            self._time_bars_origin_offset = pd.Timedelta(self._time_bars_origin_offset)

    def __str__(self):
        return f"{type(self).__name__}(interval_ns={self.interval_ns}, next_close_ns={self.next_close_ns})"

    cpdef void set_clock(self, Clock clock):
        self._clock = clock

    cpdef void start_timer(self):
        # Computing start_time
        cdef datetime now = self._clock.utc_now()
        cdef datetime start_time = self.get_start_time(now)
        start_time += timedelta(microseconds=self._bar_build_delay)

        # Closing a partial bar at the transition from historical to backtest data
        cdef bint fire_immediately = (start_time == now)

        self._skip_first_non_full_bar = self._skip_first_non_full_bar and now > start_time

        if self.bar_type.spec.aggregation not in (BarAggregation.MONTH, BarAggregation.YEAR):
            self._clock.set_timer(
                name=self._timer_name,
                interval=self.interval,
                start_time=start_time,
                stop_time=None,
                callback=self._build_bar,
                allow_past=True,
                fire_immediately=fire_immediately,
            )

            if fire_immediately:
                self.next_close_ns = dt_to_unix_nanos(start_time)
            else:
                self.next_close_ns = dt_to_unix_nanos(start_time + self.interval)

            self.stored_open_ns = self.next_close_ns - self.interval_ns
        else:
            # The monthly/yearly alert time is defined iteratively at each alert time as there is no regular interval
            if self.bar_type.spec.aggregation == BarAggregation.MONTH:
                alert_time = start_time + (pd.DateOffset(months=self.bar_type.spec.step) if not fire_immediately else pd.Timedelta(0))
            elif self.bar_type.spec.aggregation == BarAggregation.YEAR:
                alert_time = start_time + (pd.DateOffset(years=self.bar_type.spec.step) if not fire_immediately else pd.Timedelta(0))
            else:
                alert_time = start_time

            self._clock.set_time_alert(
                name=self._timer_name,
                alert_time=alert_time,
                callback=self._build_bar,
                override=True,
                allow_past=True,
            )
            self.next_close_ns = alert_time.value
            self.stored_open_ns = start_time.value

        self._log.debug(f"Started timer {self._timer_name}, {start_time=}, {self.historical_mode=}, "
                        f"{fire_immediately=}, {start_time=}, {now=}, {self._bar_build_delay=}")

    cpdef void stop_timer(self):
        self._clock.cancel_timer(str(self.bar_type))

    def get_start_time(self, now: datetime) -> datetime:
        """
        Return the start time for the aggregator's next bar.
        """
        step = self.bar_type.spec.step
        aggregation = self.bar_type.spec.aggregation

        if aggregation == BarAggregation.MILLISECOND:
            start_time = find_closest_smaller_time(now, self._time_bars_origin_offset, pd.Timedelta(milliseconds=step))
        elif aggregation == BarAggregation.SECOND:
            start_time = find_closest_smaller_time(now, self._time_bars_origin_offset, pd.Timedelta(seconds=step))
        elif aggregation == BarAggregation.MINUTE:
            start_time = find_closest_smaller_time(now, self._time_bars_origin_offset, pd.Timedelta(minutes=step))
        elif aggregation == BarAggregation.HOUR:
            start_time = find_closest_smaller_time(now, self._time_bars_origin_offset, pd.Timedelta(hours=step))
        elif aggregation == BarAggregation.DAY:
            start_time = find_closest_smaller_time(now, self._time_bars_origin_offset, pd.Timedelta(days=step))
        elif aggregation == BarAggregation.WEEK:
            start_time = (now - pd.Timedelta(days=now.dayofweek)).floor(freq="d")

            if self._time_bars_origin_offset is not None:
                start_time += self._time_bars_origin_offset

            if now < start_time:
                start_time -= pd.Timedelta(weeks=step)
        elif aggregation == BarAggregation.MONTH:
            start_time = (now - pd.DateOffset(months=now.month - 1, days=now.day - 1)).floor(freq="d")

            if self._time_bars_origin_offset is not None:
                start_time += self._time_bars_origin_offset

            if now < start_time:
                start_time -= pd.DateOffset(years=1)

            while start_time <= now:
                start_time += pd.DateOffset(months=step)

            start_time -= pd.DateOffset(months=step)
        elif aggregation == BarAggregation.YEAR:
            start_time = (now - pd.DateOffset(months=now.month - 1, days=now.day - 1)).floor(freq="d")

            if self._time_bars_origin_offset is not None:
                start_time += self._time_bars_origin_offset

            if now < start_time:
                start_time -= pd.DateOffset(years=step)
        else:  # pragma: no cover (design-time error)
            raise ValueError(
                f"Aggregation type not supported for time bars, "
                f"was {bar_aggregation_to_str(aggregation)}",
            )

        return start_time

    cdef uint64_t _get_interval_ns(self):
        return self._get_interval().value

    def _get_interval(self) -> pd.Timedelta:
        cdef BarAggregation aggregation = self.bar_type.spec.aggregation
        cdef int step = self.bar_type.spec.step

        if aggregation == BarAggregation.MILLISECOND:
            return pd.Timedelta(milliseconds=(1 * step))
        elif aggregation == BarAggregation.SECOND:
            return pd.Timedelta(seconds=(1 * step))
        elif aggregation == BarAggregation.MINUTE:
            return pd.Timedelta(minutes=(1 * step))
        elif aggregation == BarAggregation.HOUR:
            return pd.Timedelta(hours=(1 * step))
        elif aggregation == BarAggregation.DAY:
            return pd.Timedelta(days=(1 * step))
        elif aggregation == BarAggregation.WEEK:
            return pd.Timedelta(days=(7 * step))
        elif aggregation in (BarAggregation.MONTH, BarAggregation.YEAR):
            # not actually used
            return pd.Timedelta(days=0)
        else:
            # Design time error
            raise ValueError(
                f"Aggregation not time based, was {bar_aggregation_to_str(aggregation)}",
            )

    cdef void _apply_update(self, Price price, Quantity size, uint64_t ts_init):
        if self.historical_mode:
            self._preprocess_historical_events(ts_init)

        self._builder.update(price, size, ts_init)

        if self.historical_mode:
            self._postprocess_historical_events(ts_init)

    cdef void _apply_update_bar(self, Bar bar, Quantity volume, uint64_t ts_init):
        if self.historical_mode:
            self._preprocess_historical_events(ts_init)

        self._builder.update_bar(bar, volume, ts_init)

        if self.historical_mode:
            self._postprocess_historical_events(ts_init)

    cdef void _preprocess_historical_events(self, uint64_t ts_init):
        if self._clock.timestamp_ns() == 0:
            self._clock.set_time(ts_init)
            self.start_timer()

        # Advance this aggregator's independent clock and collect timer events
        self._historical_events = self._clock.advance_time(ts_init, set_time=True)

    cdef void _postprocess_historical_events(self, uint64_t ts_init):
        # Process timer events after data processing
        for event_handler in self._historical_events:
            self._build_bar(event_handler.event)

    cpdef void _build_bar(self, TimeEvent event):
        if not self._builder.initialized:
            return

        if not self._build_with_no_updates and self._builder.count == 0:
            return  # Do not build bar when no update

        cdef uint64_t ts_init = event.ts_event
        cdef uint64_t ts_event

        if self._is_left_open:
            ts_event = event.ts_event if self._timestamp_on_close else self.stored_open_ns
        else:
            ts_event = self.stored_open_ns

        self._build_and_send(ts_event=ts_event, ts_init=ts_init)

        # Close time becomes the next open time
        self.stored_open_ns = event.ts_event

        if self.bar_type.spec.aggregation == BarAggregation.MONTH:
            alert_time = unix_nanos_to_dt(event.ts_event) + pd.DateOffset(months=self.bar_type.spec.step)
            self._clock.set_time_alert(
                name=self._timer_name,
                alert_time=alert_time,
                callback=self._build_bar,
                override=True,
            )
            self.next_close_ns = dt_to_unix_nanos(alert_time)
        elif self.bar_type.spec.aggregation == BarAggregation.YEAR:
            alert_time = unix_nanos_to_dt(event.ts_event) + pd.DateOffset(years=self.bar_type.spec.step)
            self._clock.set_time_alert(
                name=self._timer_name,
                alert_time=alert_time,
                callback=self._build_bar,
                override=True,
            )
            self.next_close_ns = dt_to_unix_nanos(alert_time)
        else:
            # On receiving this event, timer should now have a new `next_time_ns`
            self.next_close_ns = self._clock.next_time_ns(self._timer_name)

    cdef void _build_and_send(self, uint64_t ts_event, uint64_t ts_init):
        if self._skip_first_non_full_bar:
            self._builder.reset()
            self._skip_first_non_full_bar = False
        else:
            BarAggregator._build_and_send(self, ts_event, ts_init)


def find_closest_smaller_time(
    now: pd.Timestamp,
    daily_time_origin: pd.Timedelta,
    period: pd.Timedelta
) -> pd.Timestamp:
    """Find the closest bar start_time <= now"""
    day_start = now.floor(freq="d")
    base_time = day_start + daily_time_origin

    time_difference = now - base_time
    num_periods = time_difference // period

    closest_time = base_time + num_periods * period

    return closest_time
