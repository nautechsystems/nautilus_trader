from collections import deque

from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class OnBalanceVolume(Indicator):
    """
    An indicator which calculates the momentum of relative positive or negative
    volume.

    Parameters
    ----------
    period : int
        The period for the indicator, zero indicates no window (>= 0).

    Raises
    ------
    ValueError
        If `period` is negative (< 0).
    """

    period: int
    value: float
    _obv: deque

    def __init__(self, period: int = 0) -> None: ...
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
        open: float,
        close: float,
        volume: float,
    ) -> None:
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        open : double
            The high price.
        close : double
            The low price.
        volume : double
            The close price.

        """
        ...
    def _reset(self) -> None: ...
