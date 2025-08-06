from collections import deque
from datetime import datetime

from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class Swings(Indicator):

    period: int
    _high_inputs: deque
    _low_inputs: deque
    direction: int
    changed: bool
    high_datetime: datetime.datetime | None
    low_datetime: datetime.datetime | None
    high_price: float
    low_price: float
    length: float
    duration: int
    since_high: int
    since_low: int

    def __init__(self, period: int) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(self, high: float, low: float, timestamp: datetime) -> None: ...
    def _reset(self) -> None: ...

