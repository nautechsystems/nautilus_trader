from nautilus_trader.indicators.average.ma_factory import MovingAverageType

from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.indicators.base.indicator import Indicator


class DirectionalMovement(Indicator):
    """
    Two oscillators that capture positive and negative trend movement.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    ma_type : MovingAverageType
        The moving average type for the indicator (cannot be None).
    """

    def __init__(
        self,
        period: int,
        ma_type: MovingAverageType = MovingAverageType.EXPONENTIAL,
    ) -> None: ...
    @property
    def period(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def pos(self) -> float: ...
    @property
    def neg(self) -> float: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(
        self,
        high: float,
        low: float,
    ) -> None: ...
    def reset(self) -> None: ...