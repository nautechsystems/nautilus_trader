from collections import deque

from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class EfficiencyRatio(Indicator):
    """
    An indicator which calculates the efficiency ratio across a rolling window.
    The Kaufman Efficiency measures the ratio of the relative market speed in
    relation to the volatility, this could be thought of as a proxy for noise.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (>= 2).

    Raises
    ------
    ValueError
        If `period` is not >= 2.
    """

    period: int
    value: float

    _inputs: deque
    _deltas: deque

    def __init__(self, period: int) -> None: ...
    def handle_bar(self, bar: Bar) -> None:
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar.

        """
        ...
    def update_raw(self, price: float) -> None:
        """
        Update the indicator with the given price.

        Parameters
        ----------
        price : double
            The update price.

        """
        ...
    def _reset(self) -> None: ...
