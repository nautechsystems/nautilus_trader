from nautilus_trader.indicators.average.moving_average import MovingAverageType
from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import RelativeStrengthIndex as Indicator


class RelativeStrengthIndex(Indicator):
    """
    An indicator which calculates a relative strength index (RSI) across a rolling window.

    Parameters
    ----------
    ma_type : int
        The moving average type for average gain/loss.
    period : MovingAverageType
        The rolling window period for the indicator.

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
    """

    def __init__(
        self,
        period: int,
        ma_type: MovingAverageType = MovingAverageType.EXPONENTIAL,
    ) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(self, value: float) -> None: ...