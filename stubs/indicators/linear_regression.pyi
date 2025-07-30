from collections import deque
from statistics import mean

import numpy as np

from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import Indicator


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

    def __init__(self, period: int = 0) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def slope(self) -> float: ...
    @property
    def intercept(self) -> float: ...
    @property
    def degree(self) -> float: ...
    @property
    def cfo(self) -> float: ...
    @property
    def r2(self) -> float: ...
    @property
    def value(self) -> float: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
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
    def reset(self) -> None: ...