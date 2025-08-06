from stubs.indicators.average.moving_average import MovingAverageType
from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class CommodityChannelIndex(Indicator):

    period: int
    scalar: float
    value: float

    def __init__(
        self,
        period: int,
        scalar: float = 0.015,
        ma_type: MovingAverageType = ...,
    ) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(
        self,
        high: float,
        low: float,
        close: float,
    ) -> None: ...
    def _reset(self) -> None: ...
