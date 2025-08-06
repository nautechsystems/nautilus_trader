from stubs.indicators.average.moving_average import MovingAverageType
from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class AverageTrueRange(Indicator):

    period: int
    value: float

    def __init__(
        self,
        period: int,
        ma_type: MovingAverageType = ...,
        use_previous: bool = True,
        value_floor: float = 0,
    ) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(
        self,
        high: float,
        low: float,
        close: float,
    ) -> None: ...
    def _reset(self) -> None: ...
