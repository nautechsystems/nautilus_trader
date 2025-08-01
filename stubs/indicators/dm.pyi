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

    period: int
    pos: float
    neg: float

    def __init__(
        self,
        period: int,
        ma_type: MovingAverageType = MovingAverageType.EXPONENTIAL,
    ) -> None: ...
    def handle_bar(self, bar: Bar) -> None:
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar.

        """
        ...
    def update_raw(
        self,
        high: float,
        low: float,
    ) -> None:
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        high : double
            The high price.
        low : double
            The low price.

        """
        ...
    def _reset(self) -> None:
        """
        Reset the indicator.

        All stateful fields are reset to their initial value.
        """
        ...
