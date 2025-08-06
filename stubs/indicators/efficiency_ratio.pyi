from collections import deque

from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class EfficiencyRatio(Indicator):

    period: int
    value: float

    _inputs: deque
    _deltas: deque

    def __init__(self, period: int) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(self, price: float) -> None: ...
    def _reset(self) -> None: ...
