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

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.timer cimport TimeEvent
from nautilus_trader.common.timer cimport TestTimer
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarSpecification
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation cimport bar_aggregation_to_string
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
        self.last_update = None
        self.initialized = False
        self.use_previous_close = use_previous_close
        self.count = 0

        self._last_close = None
        self._open = None
        self._high = None
        self._low = None
        self._close = None
        self._volume = Quantity()

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"bar_spec={self.bar_spec},"
                f"{self._open},"
                f"{self._high},"
                f"{self._low},"
                f"{self._close},"
                f"{self._volume})")

    cpdef void handle_quote_tick(self, QuoteTick tick) except *:
        """
        Update the builder with the given tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick to update with.

        """
        Condition.not_none(tick, "tick")

        if self.last_update is not None and tick.timestamp < self.last_update:
            return  # Previously handled tick

        self._update(
            price=tick.extract_price(self.bar_spec.price_type),
            volume=tick.extract_volume(self.bar_spec.price_type),
            timestamp=tick.timestamp,
        )

    cpdef void handle_trade_tick(self, TradeTick tick) except *:
        """
        Update the builder with the given tick.

        Parameters
        ----------
        tick : TradeTick
            The tick to update with.

        """
        Condition.not_none(tick, "tick")

        if self.last_update and tick.timestamp < self.last_update:
            return  # Previously handled tick

        self._update(
            price=tick.price,
            volume=tick.size,
            timestamp=tick.timestamp,
        )

    cpdef Bar build(self, datetime close_time=None):
        """
        Return a bar from the internal properties.

        Parameters
        ----------
        close_time : datetime, optional
            The closing time for the bar (if None will be last updated time).

        Returns
        -------
        Bar

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
            timestamp=close_time,
        )

        self._last_close = self._close
        self._reset()
        return bar

    cdef void _update(self, Price price, Quantity volume, datetime timestamp) except *:
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
        self._volume = Quantity(self._volume + volume)
        self.count += 1
        self.last_update = timestamp

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

        self._volume = Quantity()
        self.count = 0


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
        """Abstract method."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void handle_trade_tick(self, TradeTick tick) except *:
        """Abstract method."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void _handle_bar(self, Bar bar) except *:
        self._handler(self.bar_type, bar)


cdef class TickBarAggregator(BarAggregator):
    """
    Provides a means of building tick bars from ticks.
    """

    def __init__(
            self,
            BarType bar_type not None,
            handler not None,
            Logger logger not None,
    ):
        """
        Initialize a new instance of the `TickBarBuilder` class.

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

        self._builder.handle_quote_tick(tick)
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

        self._builder.handle_trade_tick(tick)
        self._check_bar_builder()

    cdef inline void _check_bar_builder(self) except *:
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

        if self._clock.is_test_clock:
            if self.next_close < tick.timestamp:
                # Build bar first, then update
                self._build_bar(self.next_close)
                self._builder.handle_quote_tick(tick)
                return
            elif self.next_close == tick.timestamp:
                # Update first, then build bar
                self._builder.handle_quote_tick(tick)
                self._build_bar(self.next_close)
                return

        self._builder.handle_quote_tick(tick)

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
                self._builder.handle_trade_tick(tick)
                return
            elif self.next_close == tick.timestamp:
                # Update first, then build bar
                self._builder.handle_trade_tick(tick)
                self._build_bar(self.next_close)
                return

        self._builder.handle_trade_tick(tick)

    cpdef void stop(self) except *:
        """
        Stop the bar aggregator.
        """
        self._clock.cancel_timer(str(self.bar_type))

    # noinspection: CPython datetime attributes
    # noinspection PyUnresolvedReferences
    cpdef datetime get_start_time(self):
        cdef datetime now = self._clock.utc_now()
        if self.bar_type.spec.aggregation == BarAggregation.SECOND:
            return datetime(
                year=now.year,
                month=now.month,
                day=now.day,
                hour=now.hour,
                minute=now.minute,
                second=now.second,
                tzinfo=now.tzinfo,
            )
        elif self.bar_type.spec.aggregation == BarAggregation.MINUTE:
            return datetime(
                year=now.year,
                month=now.month,
                day=now.day,
                hour=now.hour,
                minute=now.minute,
                tzinfo=now.tzinfo,
            )
        elif self.bar_type.spec.aggregation == BarAggregation.HOUR:
            return datetime(
                year=now.year,
                month=now.month,
                day=now.day,
                hour=now.hour,
                tzinfo=now.tzinfo,
            )
        elif self.bar_type.spec.aggregation == BarAggregation.DAY:
            return datetime(
                year=now.year,
                month=now.month,
                day=now.day,
            )
        else:
            # Design time error
            raise ValueError(f"Aggregation not a time, "
                             f"was {bar_aggregation_to_string(self.bar_type.spec.aggregation)}")

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
                             f"was {bar_aggregation_to_string(aggregation)}")

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
        cdef Bar bar
        try:
            if self._builder.use_previous_close and not self._builder.initialized:
                self._log.error(f"Cannot build {self.bar_type} (no prices received).")
                return

            bar = self._builder.build(event.timestamp)
            self._handle_bar(bar)
        except ValueError as ex:
            # Bar was somehow malformed
            self._log.exception(ex)
            return
