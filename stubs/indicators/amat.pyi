from typing import ClassVar

from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import MovingAverageType
from stubs.indicators.base.indicator import Indicator

class ArcherMovingAveragesTrends(Indicator):
    """
    Archer Moving Averages Trends indicator.

    Parameters
    ----------
    fast_period : int
        The period for the fast moving average (> 0).
    slow_period : int
        The period for the slow moving average (> 0 & > fast_sma).
    signal_period : int
        The period for lookback price array (> 0).
    ma_type : MovingAverageType
        The moving average type for the calculations.

    References
    ----------
    https://github.com/twopirllc/pandas-ta/blob/bc3b292bf1cc1d5f2aba50bb750a75209d655b37/pandas_ta/trend/amat.py
    """

    fast_period: ClassVar[int]
    slow_period: ClassVar[int]
    signal_period: ClassVar[int]
    long_run: ClassVar[int]
    short_run: ClassVar[int]

    def __init__(
        self,
        fast_period: int,
        slow_period: int,
        signal_period: int,
        ma_type: MovingAverageType = MovingAverageType.EXPONENTIAL,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def fast_period(self) -> int: ...
    @property
    def slow_period(self) -> int: ...
    @property
    def signal_period(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def long_run(self) -> bool: ...
    @property
    def short_run(self) -> bool: ...
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
        Update the indicator with the given close price value.

        Parameters
        ----------
        close : double
            The close price.

        """
        ...
    def reset(self) -> None: ...
