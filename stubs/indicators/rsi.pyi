from stubs.indicators.average.moving_average import MovingAverageType
from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

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
        ma_type: MovingAverageType = ...,
    ) -> None: ...
    def handle_bar(self, bar: Bar) -> None:
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar.

        """
    def update_raw(self, value: float) -> None:
        """
        Update the indicator with the given value.

        Parameters
        ----------
        value : double
            The update value.

        """
    def _reset(self) -> None: ...
