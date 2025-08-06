from stubs.indicators.average.moving_average import MovingAverageType
from stubs.indicators.base.indicator import Indicator
from stubs.indicators.keltner_channel import KeltnerChannel
from stubs.model.data import Bar

class KeltnerPosition(Indicator):

    period: int
    k_multiplier: float
    value: float
    _kc: KeltnerChannel

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
