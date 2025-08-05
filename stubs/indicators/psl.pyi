from stubs.indicators.average.moving_average import MovingAverageType
from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class PsychologicalLine(Indicator):
    """
    The Psychological Line is an oscillator-type indicator that compares the
    number of the rising periods to the total number of periods. In other
    words, it is the percentage of bars that close above the previous
    bar over a given period.

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
    def update_raw(self, close: float) -> None:
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        close : double
            The close price.

        """
    def _reset(self) -> None: ...

