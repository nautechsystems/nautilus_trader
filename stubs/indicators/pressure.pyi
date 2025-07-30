from nautilus_trader.core.nautilus_pyo3 import AverageTrueRange
from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import MovingAverageType
from stubs.indicators.average.moving_average import MovingAverage

class Pressure(Indicator):
    """
    An indicator which calculates the relative volume (multiple of average volume)
    to move the market across a relative range (multiple of ATR).

    Parameters
    ----------
    period : int
        The period for the indicator (> 0).
    ma_type : MovingAverageType
        The moving average type for the calculations.
    atr_floor : double
        The ATR floor (minimum) output value for the indicator (>= 0.).

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
    ValueError
        If `atr_floor` is negative (< 0).
    """

    period: int
    value: float
    value_cumulative: float
    _atr: AverageTrueRange
    _average_volume: MovingAverage

    def __init__(
        self,
        period: int,
        ma_type: MovingAverageType = ...,
        atr_floor: float = 0,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    @property
    def value_cumulative(self) -> float: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(
        self,
        high: float,
        low: float,
        close: float,
        volume: float,
    ) -> None: ...
    def reset(self) -> None: ...
