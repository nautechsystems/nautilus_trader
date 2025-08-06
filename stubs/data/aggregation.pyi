from collections.abc import Callable
from datetime import datetime
from datetime import timedelta
from decimal import Decimal

import pandas as pd

from stubs.common.component import Clock
from stubs.common.component import Logger
from stubs.common.component import TimeEvent
from stubs.model.data import Bar
from stubs.model.data import BarType
from stubs.model.data import QuoteTick
from stubs.model.data import TradeTick
from stubs.model.instruments.base import Instrument
from stubs.model.objects import Price
from stubs.model.objects import Quantity

class BarBuilder:

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
    def set_partial(self, partial_bar: Bar) -> None: ...
    def update(self, price: Price, size: Quantity, ts_event: int) -> None: ...
    def update_bar(self, bar: Bar, volume: Quantity, ts_init: int) -> None: ...
    def reset(self) -> None: ...
    def build_now(self) -> Bar: ...
    def build(self, ts_event: int, ts_init: int) -> Bar: ...

class BarAggregator:

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
    def handle_quote_tick(self, tick: QuoteTick) -> None: ...
    def handle_trade_tick(self, tick: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def set_partial(self, partial_bar: Bar) -> None: ...

class TickBarAggregator(BarAggregator):
    def __init__(self, instrument: Instrument, bar_type: BarType, handler: Callable[[Bar], None]) -> None: ...

class VolumeBarAggregator(BarAggregator):
    def __init__(self, instrument: Instrument, bar_type: BarType, handler: Callable[[Bar], None]) -> None: ...

class ValueBarAggregator(BarAggregator):

    _cum_value: Decimal

    def __init__(self, instrument: Instrument, bar_type: BarType, handler: Callable[[Bar], None]) -> None: ...
    def get_cumulative_value(self) -> Decimal: ...

class TimeBarAggregator(BarAggregator):

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
    def get_start_time(self, now: datetime) -> datetime: ...
    def _set_build_timer(self) -> None: ...
    def stop(self) -> None: ...
    def _start_batch_time(self, time_ns: int): ...
    def _build_bar(self, event: TimeEvent) -> None: ...

