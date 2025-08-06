from stubs.indicators.average.moving_average import MovingAverageType
from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class ArcherMovingAveragesTrends(Indicator):

    fast_period: int
    slow_period: int
    signal_period: int
    long_run: int
    short_run: int

    def __init__(
        self,
        fast_period: int,
        slow_period: int,
        signal_period: int,
        ma_type: MovingAverageType = ...,
    ) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(self, close: float) -> None: ...
    def _reset(self) -> None: ...

