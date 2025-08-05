from collections import deque

from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class RateOfChange(Indicator):
    """
    An indicator which calculates the rate of change of price over a defined period.
    The return output can be simple or log.

    Parameters
    ----------
    period : int
        The period for the indicator.
    use_log : bool
        Use log returns for value calculation.

    Raises
    ------
    ValueError
        If `period` is not > 1.
    """

    period: int
    value: float
    _use_log: bool
    _prices: deque

    def __init__(self, period: int, use_log: bool = False) -> None: ...
    def handle_bar(self, bar: Bar) -> None:
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar.

        """
    def update_raw(self, price: float) -> None:
        """
        Update the indicator with the given price.

        Parameters
        ----------
        price : double
            The update price.

        """
    def _reset(self) -> None: ...

