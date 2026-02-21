# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
from libc.math cimport fabs
from libc.stdint cimport uint64_t

from datetime import timedelta

from nautilus_trader.core.datetime import unix_nanos_to_dt

from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.common.component cimport TimeEvent
from nautilus_trader.common.data_topics cimport TopicCache
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport dt_to_unix_nanos
from nautilus_trader.core.rust.core cimport millis_to_nanos
from nautilus_trader.core.rust.core cimport secs_to_nanos
from nautilus_trader.core.rust.model cimport FIXED_SCALAR
from nautilus_trader.core.rust.model cimport AggressorSide
from nautilus_trader.core.rust.model cimport InstrumentClass
from nautilus_trader.core.rust.model cimport QuantityRaw
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport BarAggregation
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.functions cimport bar_aggregation_to_str
from nautilus_trader.model.greeks cimport GreeksCalculator
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport generic_spread_id_to_list
from nautilus_trader.model.identifiers cimport is_generic_spread_id
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity

from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.datetime import unix_nanos_to_iso8601


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
        self._close = None

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

    cdef bint _is_below_min_size(self, double size, int precision):
        return Quantity(size, precision=precision)._mem.raw == 0

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

            # Clamp to minimum representable size to avoid zero-volume bars
            if self._is_below_min_size(size_diff, size._mem.precision):
                if self._is_below_min_size(size_update, size._mem.precision):
                    break
                size_diff = Decimal(10) ** -size._mem.precision

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

            # Clamp to minimum representable size to avoid zero-volume bars
            if self._is_below_min_size(volume_diff, volume._mem.precision):
                if self._is_below_min_size(volume_update, volume._mem.precision):
                    break
                volume_diff = Decimal(10) ** -volume._mem.precision

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

                # Clamp to minimum representable size to avoid zero-volume bars
                if self._is_below_min_size(size_chunk, tick.size.precision):
                    if self._is_below_min_size(size_remaining, tick.size.precision):
                        break
                    size_chunk = 10.0 ** -tick.size.precision
                    value_chunk = price_f64 * size_chunk

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

                # Clamp to minimum representable size to avoid zero-volume bars
                if self._is_below_min_size(size_chunk, tick.size.precision):
                    if self._is_below_min_size(size_remaining, tick.size.precision):
                        break
                    size_chunk = 10.0 ** -tick.size.precision
                    value_to_flatten = price_f64 * size_chunk

                self._apply_update(
                    tick.price,
                    Quantity(size_chunk, precision=tick.size.precision),
                    tick.ts_init,
                )
                self._imbalance_value += side_sign * value_to_flatten

                # Min-size clamp can overshoot past threshold
                if abs(self._imbalance_value) >= self._step_value:
                    self._build_now_and_send()
                    self._imbalance_value = 0.0
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

            # Clamp to minimum representable size to avoid zero-volume bars
            if self._is_below_min_size(size_chunk, tick.size.precision):
                if self._is_below_min_size(size_remaining, tick.size.precision):
                    break
                size_chunk = 10.0 ** -tick.size.precision

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
        self._timer_name = f"time_bar_{self.bar_type}"
        self.interval = self._get_interval()
        self.interval_ns = self._get_interval_ns()
        self.stored_open_ns = 0
        self.next_close_ns = 0
        self.first_close_ns = 0
        self.historical_mode = False
        self._historical_events = []
        self._historical_event_at_ts_init = None

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

        # Calculate the next close time based on aggregation type
        cdef datetime close_time
        if fire_immediately:
            close_time = start_time
        elif self.bar_type.spec.aggregation == BarAggregation.MONTH:
            close_time = start_time + pd.DateOffset(months=self.bar_type.spec.step)
        elif self.bar_type.spec.aggregation == BarAggregation.YEAR:
            close_time = start_time + pd.DateOffset(years=self.bar_type.spec.step)
        else:
            close_time = start_time + self.interval

        self.next_close_ns = dt_to_unix_nanos(close_time)

        # The stored open time needs to be defined as a subtraction with respect to the first closing time
        if self.bar_type.spec.aggregation == BarAggregation.MONTH:
            self.stored_open_ns = dt_to_unix_nanos(close_time - pd.DateOffset(months=self.bar_type.spec.step))
        elif self.bar_type.spec.aggregation == BarAggregation.YEAR:
            self.stored_open_ns = dt_to_unix_nanos(close_time - pd.DateOffset(years=self.bar_type.spec.step))
        else:
            self.stored_open_ns = self.next_close_ns - self.interval_ns

        if self._skip_first_non_full_bar:
            self.first_close_ns = self.next_close_ns

        if self.bar_type.spec.aggregation in (BarAggregation.MONTH, BarAggregation.YEAR):
            # The monthly/yearly alert time is defined iteratively at each alert time as there is no regular interval
            self._clock.set_time_alert(
                name=self._timer_name,
                alert_time=close_time,
                callback=self._build_bar,
                override=True,
                allow_past=True,
            )
        else:
            self._clock.set_timer(
                name=self._timer_name,
                interval=self.interval,
                start_time=start_time,
                stop_time=None,
                callback=self._build_bar,
                allow_past=True,
                fire_immediately=fire_immediately,
            )

        self._log.debug(f"[start_timer] fire_immediately={fire_immediately}, "
                        f"_skip_first_non_full_bar={self._skip_first_non_full_bar}, "
                        f"now={now}, start_time={start_time}, "
                        f"first_close_ns={unix_nanos_to_dt(self.first_close_ns)}, "
                        f"next_close_ns={unix_nanos_to_dt(self.next_close_ns)}")

    cpdef void stop_timer(self):
        cdef str timer_name = str(self.bar_type)
        if timer_name in self._clock.timer_names:
            self._clock.cancel_timer(timer_name)

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
            self._pre_process_historical_events(ts_init)

        self._builder.update(price, size, ts_init)

        if self.historical_mode:
            self._post_process_historical_events()

    cdef void _apply_update_bar(self, Bar bar, Quantity volume, uint64_t ts_init):
        if self.historical_mode:
            self._pre_process_historical_events(ts_init)

        self._builder.update_bar(bar, volume, ts_init)

        if self.historical_mode:
            self._post_process_historical_events()

    cdef void _pre_process_historical_events(self, uint64_t ts_init):
        if self._clock.timestamp_ns() == 0:
            self._clock.set_time(ts_init)
            self.start_timer()

        # Advance this aggregator's independent clock and collect timer events
        event_handlers = self._clock.advance_time(ts_init, set_time=True)

        # Process timer events
        for event_handler in event_handlers:
            if event_handler.event.ts_event == ts_init:
                self._historical_event_at_ts_init = event_handler
                continue

            self._build_bar(event_handler.event)

    cdef void _post_process_historical_events(self):
        # Process timer events
        if self._historical_event_at_ts_init:
            self._build_bar(self._historical_event_at_ts_init.event)
            self._historical_event_at_ts_init = None

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
        if self._skip_first_non_full_bar and ts_init <= self.first_close_ns:
            self._builder.reset()
        else:
            # Set _skip_first_non_full_bar to False for transition from historical to live data
            self._skip_first_non_full_bar = False
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


cdef class SpreadQuoteAggregator:
    """
    Provides a spread quote generator for creating synthetic quotes from leg instruments.

    The generator receives quote ticks from leg instruments via handler callbacks and generates
    synthetic quotes for the spread instrument. Pricing logic differs by instrument type:

    - **Futures spreads**: Calculates weighted bid/ask prices based on leg ratios (positive ratios
      use bid/ask directly, negative ratios invert bid/ask).
    - **Option spreads**: Uses vega-weighted spread calculation to determine bid/ask spreads,
      then applies to the weighted mid-price based on leg ratios.

    The aggregator requires quotes from all legs before building a spread quote. It can operate
    in two modes:

    1. **Quote-driven mode** (`update_interval_seconds=None`): Receives quote tick updates via handler
       and builds spread quotes immediately when all legs have received quotes. This is the default
       and recommended mode for most use cases.

    2. **Timer-driven mode** (`update_interval_seconds=int`): Uses a periodic timer to read quotes
       from internal state and build spread quotes at regular intervals. In historical mode, timer
       events are processed when quotes arrive, ensuring all quotes for a given timestamp are
       received before processing timer events for that timestamp.

    In historical mode, the aggregator advances the provided clock independently with incoming
    data timestamps, similar to TimeBarAggregator. Timer events are generated by advancing the
    clock and are processed only when all legs have received quotes for the corresponding timestamp.

    Parameters
    ----------
    spread_instrument : Instrument
        The spread instrument to generate quotes for.
    handler : Callable[[QuoteTick], None]
        The quote handler callback that receives generated spread quotes.
    greeks_calculator : GreeksCalculator
        The greeks calculator for calculating option greeks (required for option spreads).
    clock : Clock
        The clock for timing operations and timer management.
    historical : bool
        Whether the aggregator is processing historical data. When True, the clock is advanced
        independently with incoming data timestamps.
    update_interval_seconds : int | None, default None
        The interval in seconds for timer-driven quote building. If None, uses quote-driven mode
        (builds immediately when all legs have quotes). If an integer, uses timer-driven mode
        (reads from internal state at the specified interval).
    quote_build_delay : int, default 0
        The time delay (microseconds) before building and emitting a quote.

    Raises
    ------
    ValueError
        If `spread_instrument` has one or fewer legs.
    """

    def __init__(
        self,
        Instrument spread_instrument not None,
        handler not None: Callable[[QuoteTick], None],
        GreeksCalculator greeks_calculator not None,
        Clock clock not None,
        bint historical,
        object update_interval_seconds = None,
        int quote_build_delay = 0,
    ):
        self._handler = handler
        self._clock = clock
        self._log = Logger(name=f"{type(self).__name__}")

        self._spread_instrument = spread_instrument
        self._spread_instrument_id = spread_instrument.id

        # Get spread legs from instrument
        self._legs = spread_instrument.legs()
        if not self._legs or len(self._legs) <= 1:
            raise ValueError(f"Spread instrument {spread_instrument.id} must have more than one leg")

        self._greeks_calculator = greeks_calculator

        self._leg_ids = [leg[0] for leg in self._legs]
        self._ratios = np.array([leg[1] for leg in self._legs])
        self._n_legs = len(self._legs)
        self._mid_prices = np.zeros(self._n_legs)
        self._bid_prices = np.zeros(self._n_legs)
        self._ask_prices = np.zeros(self._n_legs)
        self._vegas = np.zeros(self._n_legs)
        self._bid_ask_spreads = np.zeros(self._n_legs)
        self._bid_sizes = np.zeros(self._n_legs)
        self._ask_sizes = np.zeros(self._n_legs)
        self._last_quotes = {}

        self._is_futures_spread = self._spread_instrument.instrument_class == InstrumentClass.FUTURES_SPREAD
        self.historical_mode = historical
        self._update_interval_seconds = update_interval_seconds
        self._quote_build_delay = quote_build_delay
        self.is_running = False
        self._historical_events = []

        # Timers on a same clock execute first based on their timer name
        # "spread_quote_..." < "time_bar_..."
        self._timer_name = f"spread_quote_{self._spread_instrument_id}"
        self._has_update = False

    cpdef void set_historical_mode(self, bint historical_mode, handler: Callable[[QuoteTick], None], GreeksCalculator greeks_calculator):
        Condition.callable(handler, "handler")
        Condition.not_none(greeks_calculator, "greeks_calculator")

        self.historical_mode = historical_mode
        self._handler = handler
        self._greeks_calculator = greeks_calculator

    cpdef void set_running(self, bint is_running):
        self.is_running = is_running

    cpdef void set_clock(self, Clock clock):
        self._clock = clock

    cpdef void start_timer(self):
        if self._update_interval_seconds is None:
            return

        cdef datetime now = self._clock.utc_now()
        start_time = find_closest_smaller_time(now, pd.Timedelta(0), pd.Timedelta(seconds=<int>self._update_interval_seconds))
        start_time += timedelta(microseconds=self._quote_build_delay)

        # Determine if we should fire immediately (if start_time equals now)
        cdef bint fire_immediately = (start_time == now)

        self._clock.set_timer(
            name=self._timer_name,
            interval=timedelta(seconds=<int>self._update_interval_seconds),
            callback=self._build_and_send_quote_callback,
            start_time=start_time,
            stop_time=None,   # Run indefinitely
            allow_past=True,  # Allow past start times
            fire_immediately=fire_immediately,
        )

    cpdef void stop_timer(self):
        if self._update_interval_seconds is None:
            return

        if self._timer_name in self._clock.timer_names:
            self._clock.cancel_timer(self._timer_name)

    cpdef void handle_quote_tick(self, QuoteTick tick):
        if self._update_interval_seconds is not None and self.historical_mode:
            self._process_historical_events(tick.ts_init)

        self._last_quotes[tick.instrument_id] = tick
        self._has_update = True

        self._log.debug(f"Component QuoteTick: {tick}, ts={unix_nanos_to_iso8601(tick.ts_init)}")

        if self._update_interval_seconds is None and len(self._last_quotes) == self._n_legs:
            self._build_and_send_quote(tick.ts_init)
            return

    cdef void _process_historical_events(self, uint64_t ts_init):
        if self._clock.timestamp_ns() == 0:
            self._clock.set_time(ts_init)
            self.start_timer()

        self._historical_events.extend(self._clock.advance_time(ts_init, set_time=True))
        if not self._historical_events:
            return

        # Don't process the last event and keep it if it matches ts_init
        # This is to ensure that all quotes are received for a same time
        last_event = self._historical_events[-1]
        if last_event.event.ts_event == ts_init:
            event_handlers = self._historical_events[:-1]
            self._historical_events = [last_event]
        else:
            event_handlers = self._historical_events
            self._historical_events.clear()

        if len(self._last_quotes) != self._n_legs:
            return

        # Process events if all legs have quotes
        for event_handler in event_handlers:
            self._build_and_send_quote(event_handler.event.ts_event)

    cdef void _build_and_send_quote_callback(self, TimeEvent event):
        if len(self._last_quotes) != self._n_legs:
            return

        self._build_and_send_quote(event.ts_init)

    cdef void _build_and_send_quote(self, uint64_t ts_init):
        if not self._has_update:
            return

        for idx, leg_id in enumerate(self._leg_ids):
            tick = self._last_quotes.get(leg_id)
            if tick is None:
                self._log.error(
                    f"SpreadQuoteAggregator[{self._spread_instrument_id}]: Missing quote for leg {leg_id}"
                )
                return  # Cannot build quote without all legs

            ask_price = tick.ask_price.as_double()
            bid_price = tick.bid_price.as_double()

            self._bid_prices[idx] = bid_price
            self._ask_prices[idx] = ask_price
            self._bid_sizes[idx] = tick.bid_size.as_double()
            self._ask_sizes[idx] = tick.ask_size.as_double()

            if not self._is_futures_spread:
                self._mid_prices[idx] = (ask_price + bid_price) * 0.5
                self._bid_ask_spreads[idx] = ask_price - bid_price
                greeks_data = self._greeks_calculator.instrument_greeks(
                    leg_id,
                    percent_greeks=True,
                    use_cached_greeks=True,
                    vega_time_weight_base=30,
                )
                if greeks_data is not None:
                    self._vegas[idx] = greeks_data.vega

        cdef tuple raw_bid_ask_prices
        if self._is_futures_spread:
            raw_bid_ask_prices = self._create_futures_spread_prices()
        else:
            raw_bid_ask_prices = self._create_option_spread_prices()

        spread_quote = self._create_quote_tick_from_raw_prices(raw_bid_ask_prices[0], raw_bid_ask_prices[1], ts_init)

        self._has_update = False
        self._handler(spread_quote)

    cdef tuple _create_option_spread_prices(self):
        vega_multipliers = np.divide(
            self._bid_ask_spreads,
            self._vegas,
            out=np.zeros_like(self._vegas),
            where=self._vegas != 0
        )

        # Filter out zero multipliers before taking mean
        non_zero_multipliers = vega_multipliers[vega_multipliers != 0]
        if len(non_zero_multipliers) == 0:
            self._log.warning(
                f"No vega information available for the components of {self._spread_instrument_id}. "
                f"Will generate spread quote using component quotes only. "
                f"Subscribe to some underlying price information for more precise quotes."
            )
            return self._create_futures_spread_prices()

        vega_multiplier = np.abs(non_zero_multipliers).mean()
        spread_vega = abs(np.dot(self._vegas, self._ratios))

        bid_ask_spread = spread_vega * vega_multiplier
        self._log.debug(f"{self._bid_ask_spreads=}, {self._vegas=}, {vega_multipliers=}, "
                        f"{spread_vega=}, {vega_multiplier=}, {bid_ask_spread=}")

        spread_mid_price = (self._mid_prices * self._ratios).sum()
        raw_bid_price = spread_mid_price - bid_ask_spread * 0.5
        raw_ask_price = spread_mid_price + bid_ask_spread * 0.5

        return (raw_bid_price, raw_ask_price)

    cdef tuple _create_futures_spread_prices(self):
        # Calculate spread ask: for positive ratios use ask, for negative ratios use bid
        # Calculate spread bid: for positive ratios use bid, for negative ratios use ask

        cdef double raw_ask_price = 0.0
        cdef double raw_bid_price = 0.0

        cdef int i
        for i in range(self._n_legs):
            if self._ratios[i] >= 0:
                raw_ask_price += self._ratios[i] * self._ask_prices[i]
                raw_bid_price += self._ratios[i] * self._bid_prices[i]
            else:
                raw_ask_price += self._ratios[i] * self._bid_prices[i]
                raw_bid_price += self._ratios[i] * self._ask_prices[i]

        return (raw_bid_price, raw_ask_price)

    cdef QuoteTick _create_quote_tick_from_raw_prices(self, double raw_bid_price, double raw_ask_price, uint64_t ts_init):
        # Apply tick scheme if available
        if self._spread_instrument._tick_scheme is not None:
            if raw_bid_price >= 0.:
                bid_price = self._spread_instrument._tick_scheme.next_bid_price(raw_bid_price)
            else:
                bid_price = self._spread_instrument.make_price(-self._spread_instrument._tick_scheme.next_ask_price(-raw_bid_price).as_double())

            if raw_ask_price >= 0.:
                ask_price = self._spread_instrument._tick_scheme.next_ask_price(raw_ask_price)
            else:
                ask_price = self._spread_instrument.make_price(-self._spread_instrument._tick_scheme.next_bid_price(-raw_ask_price).as_double())

            self._log.debug(f"Bid ask created using tick_scheme: {bid_price=}, {ask_price=}, {raw_bid_price=}, {raw_ask_price=}, is_futures_spread={self._is_futures_spread}")
        else:
            # Fallback to simple method if no tick scheme
            bid_price = self._spread_instrument.make_price(raw_bid_price)
            ask_price = self._spread_instrument.make_price(raw_ask_price)
            self._log.debug(f"Bid ask created: {bid_price=}, {ask_price=}, {raw_bid_price=}, {raw_ask_price=}, is_futures_spread={self._is_futures_spread}")

        # Create bid and ask sizes (use minimum of leg sizes based on ratio signs)
        cdef double min_bid_size = float('inf')
        cdef double min_ask_size = float('inf')

        cdef double abs_ratio
        cdef int i
        for i in range(self._n_legs):
            abs_ratio = fabs(self._ratios[i])
            if self._ratios[i] >= 0:
                if self._bid_sizes[i] / abs_ratio < min_bid_size:
                    min_bid_size = self._bid_sizes[i] / abs_ratio

                if self._ask_sizes[i] / abs_ratio < min_ask_size:
                    min_ask_size = self._ask_sizes[i] / abs_ratio
            else:
                if self._ask_sizes[i] / abs_ratio < min_bid_size:
                    min_bid_size = self._ask_sizes[i] / abs_ratio

                if self._bid_sizes[i] / abs_ratio < min_ask_size:
                    min_ask_size = self._bid_sizes[i] / abs_ratio

        bid_size = self._spread_instrument.make_qty(min_bid_size)
        ask_size = self._spread_instrument.make_qty(min_ask_size)

        cdef QuoteTick spread_quote = QuoteTick(
            instrument_id=self._spread_instrument_id,
            bid_price=bid_price,
            ask_price=ask_price,
            bid_size=bid_size,
            ask_size=ask_size,
            ts_event=ts_init,
            ts_init=ts_init,
        )

        return spread_quote
