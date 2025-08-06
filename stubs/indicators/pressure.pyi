from stubs.indicators.atr import AverageTrueRange
from stubs.indicators.average.ma_factory import MovingAverageFactory
from stubs.indicators.average.moving_average import MovingAverageType
from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class Pressure(Indicator):

    period: int
    value: float
    value_cumulative: float
    _atr: AverageTrueRange
    _average_volume: MovingAverageFactory

    def __init__(
        self,
        period: int,
        ma_type: MovingAverageType = ...,
        atr_floor: float = 0,
    ) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(
        self,
        high: float,
        low: float,
        close: float,
        volume: float,
    ) -> None: ...
    def _reset(self) -> None: ...
