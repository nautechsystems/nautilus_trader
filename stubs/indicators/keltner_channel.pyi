from stubs.indicators.average.moving_average import MovingAverageType
from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class KeltnerChannel(Indicator):

    period: int
    k_multiplier: float
    upper: float
    middle: float
    lower: float

    def __init__(
        self,
        period: int,
        k_multiplier: float,
        ma_type: MovingAverageType = ...,
        ma_type_atr: MovingAverageType = ...,
        use_previous: bool = True,
        atr_floor: float = 0,
    ) -> None: ...

    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(
        self,
        high: float,
        low: float,
        close: float,
    ) -> None: ...
    def _reset(self) -> None: ...
