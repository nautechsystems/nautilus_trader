from collections.abc import Callable
from datetime import datetime
from datetime import timedelta
from decimal import Decimal

import pandas as pd

from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import BarType
from nautilus_trader.core.nautilus_pyo3 import Clock
from nautilus_trader.core.nautilus_pyo3 import Instrument
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import QuoteTick
from nautilus_trader.core.nautilus_pyo3 import TradeTick
from nautilus_trader.core.nautilus_pyo3 import TimeEvent
from stubs.common.component import Logger

class BarBuilder:
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

    price_precision: int
    size_precision: int
    initialized: bool
    ts_last: int
    count: int
    volume: Quantity

    _bar_type: BarType
    _partial_set: bool
    _last_close: Price
    _open: Price
    _high: Price
    _low: Price
    _close: Price

    def __init__(self, instrument: Instrument, bar_type: BarType) -> None: ...
    def __repr__(self) -> str: ...
    def set_partial(self, partial_bar: Bar) -> None:
        """
        Set the initial values for a partially completed bar.

        This method can only be called once per instance.

        Parameters
        ----------
        partial_bar : Bar
            The partial bar with values to set.

        """
        ...
    def update(self, price: Price, size: Quantity, ts_event: int) -> None:
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
        ...
    def update_bar(self, bar: Bar, volume: Quantity, ts_init: int) -> None:
        """
        Update the bar builder.

        Parameters
        ----------
        bar : Bar
            The update Bar.

        """
        ...
    def reset(self) -> None:
        """
        Reset the bar builder.

        All stateful fields are reset to their initial value.
        """
        ...
    def build_now(self) -> Bar:
        """
        Return the aggregated bar and reset.

        Returns
        -------
        Bar

        """
        ...
    def build(self, ts_event: int, ts_init: int) -> Bar:
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
        ...

class BarAggregator:
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

    bar_type: BarType
    is_running: bool

    _handler: Callable[[Bar], None]
    _handler_backup: Callable[[Bar], None]
    _await_partial: bool
    _log: Logger
    _builder: BarBuilder
    _batch_mode: bool
    
    def __init__(self, instrument: Instrument, bar_type: BarType, handler: Callable[[Bar], None], await_partial: bool = False) -> None: ...
    def start_batch_update(self, handler: Callable[[Bar], None], time_ns: int) -> None: ...
    def _start_batch_time(self, time_ns: int): ...
    def stop_batch_update(self) -> None: ...
    def set_await_partial(self, value: bool) -> None: ...
    def handle_quote_tick(self, tick: QuoteTick) -> None:
        """
        Update the aggregator with the given tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick for the update.

        """
        ...
    def handle_trade_tick(self, tick: TradeTick) -> None:
        """
        Update the aggregator with the given tick.

        Parameters
        ----------
        tick : TradeTick
            The tick for the update.

        """
        ...
    def handle_bar(self, bar: Bar) -> None:
        """
        Update the aggregator with the given bar.

        Parameters
        ----------
        bar : Bar
            The bar for the update.

        """
        ...
    def set_partial(self, partial_bar: Bar) -> None:
        """
        Set the initial values for a partially completed bar.

        This method can only be called once per instance.

        Parameters
        ----------
        partial_bar : Bar
            The partial bar with values to set.

        """
        ...

class TickBarAggregator(BarAggregator):
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
    def __init__(self, instrument: Instrument, bar_type: BarType, handler: Callable[[Bar], None]) -> None: ...

class VolumeBarAggregator(BarAggregator):
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
    def __init__(self, instrument: Instrument, bar_type: BarType, handler: Callable[[Bar], None]) -> None: ...

class ValueBarAggregator(BarAggregator):
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

    _cum_value: Decimal

    def __init__(self, instrument: Instrument, bar_type: BarType, handler: Callable[[Bar], None]) -> None: ...
    def get_cumulative_value(self) -> Decimal:
        """
        Return the current cumulative value of the aggregator.

        Returns
        -------
        Decimal

        """
        ...

class TimeBarAggregator(BarAggregator):
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

    interval: timedelta
    interval_ns: int
    next_close_ns: int

    _clock: Clock
    _is_left_open: bool
    _timestamp_on_close: bool
    _skip_first_non_full_bar: bool
    _build_with_no_updates: bool
    _time_bars_origin_offset: pd.Timedelta | pd.DateOffset
    _bar_build_delay: int
    _timer_name: str
    _build_on_next_tick: bool
    _batch_open_ns: int
    _batch_next_close_ns: int
    _stored_open_ns: int
    _stored_close_ns: int

    def __init__(self, instrument: Instrument, bar_type: BarType, handler: Callable[[Bar], None], clock: Clock, interval_type: str = 'left-open', timestamp_on_close: bool = True, skip_first_non_full_bar: bool = False, build_with_no_updates: bool = True, time_bars_origin_offset: pd.Timedelta | pd.DateOffset = None, bar_build_delay: int = 0) -> None: ...
    def __str__(self) -> str: ...
    def get_start_time(self, now: datetime) -> datetime:
        """
        Return the start time for the aggregators next bar.

        Returns
        -------
        datetime
            The timestamp (UTC).

        """
        ...
    def _set_build_timer(self) -> None: ...
    def stop(self) -> None:
        """
        Stop the bar aggregator.
        """
        ...
    def _start_batch_time(self, time_ns: int): ...
    def _build_bar(self, event: TimeEvent) -> None: ...

