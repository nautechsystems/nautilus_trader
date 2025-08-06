from collections import deque

from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class OnBalanceVolume(Indicator):

    period: int
    value: float
    _obv: deque

    def __init__(self, period: int = 0) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(
        self,
        open: float,
        close: float,
        volume: float,
    ) -> None: ...
    def _reset(self) -> None: ...
