from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class LinearRegression(Indicator):
    """
    An indicator that calculates a simple linear regression.

    Parameters
    ----------
    period : int
        The period for the indicator.

    Raises
    ------
    ValueError
        If `period` is not greater than zero.
    """

    period: int
    slope: float
    intercept: float
    degree: float
    cfo: float
    R2: float
    value: float

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
    def update_raw(self, close: float) -> None:
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        close_price : double
            The close price.

        """
        ...
    def _reset(self) -> None: ...
