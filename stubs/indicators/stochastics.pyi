from collections import deque
from nautilus_trader.indicators.base.indicator import Indicator

class Stochastics(Indicator):
    """
    An oscillator which can indicate when an asset may be over bought or over
    sold.

    Parameters
    ----------
    period_k : int
        The period for the K line.
    period_d : int
        The period for the D line.

    Raises
    ------
    ValueError
        If `period_k` is not positive (> 0).
    ValueError
        If `period_d` is not positive (> 0).

    References
    ----------
    https://www.forextraders.com/forex-education/forex-indicators/stochastics-indicator-explained/
    """

    period_k: int
    period_d: int
    value_k: float
    value_d: float

    def __init__(self, period_k: int, period_d: int) -> None: ...
    def handle_bar(self, bar: Bar) -> None:
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar.

        """
        ...
    def update_raw(self, high: float, low: float, close: float) -> None:
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        high : double
            The high price.
        low : double
            The low price.
        close : double
            The close price.

        """
        ...
    def _reset(self) -> None: ...
