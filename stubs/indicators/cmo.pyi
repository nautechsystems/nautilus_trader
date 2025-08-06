from stubs.indicators.average.moving_average import MovingAverage
from stubs.indicators.average.moving_average import MovingAverageType
from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class ChandeMomentumOscillator(Indicator):

    period: int
    value: float
    _average_gain: MovingAverage
    _average_loss: MovingAverage
    _previous_close: float

    def __init__(
        self,
        period: int,
        ma_type: MovingAverageType = ...,
    ) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(self, close: float) -> None: ...
    def _reset(self) -> None: ...
