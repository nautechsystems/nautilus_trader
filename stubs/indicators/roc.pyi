from collections import deque

from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class RateOfChange(Indicator):

    period: int
    value: float
    _use_log: bool
    _prices: deque

    def __init__(self, period: int, use_log: bool = False) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(self, price: float) -> None: ...
    def _reset(self) -> None: ...

