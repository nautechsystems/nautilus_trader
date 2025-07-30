from nautilus_trader.indicators.average.moving_average import MovingAverageType
from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import AverageTrueRange
from nautilus_trader.core.nautilus_pyo3 import Indicator

class VolatilityRatio(Indicator):
    """
    An indicator which calculates the ratio of different ranges of volatility.
    Different moving average types can be selected for the inner ATR calculations.

    Parameters
    ----------
    fast_period : int
        The period for the fast ATR (> 0).
    slow_period : int
        The period for the slow ATR (> 0 & > fast_period).
    ma_type : MovingAverageType
        The moving average type for the ATR calculations.
    use_previous : bool
        The boolean flag indicating whether previous price values should be used.
    value_floor : double
        The floor (minimum) output value for the indicator (>= 0).

    Raises
    ------
    ValueError
        If `fast_period` is not positive (> 0).
    ValueError
        If `slow_period` is not positive (> 0).
    ValueError
        If `fast_period` is not < `slow_period`.
    ValueError
        If `value_floor` is negative (< 0).
    """

    def __init__(
        self,
        fast_period: int,
        slow_period: int,
        ma_type: MovingAverageType = MovingAverageType.SIMPLE,
        use_previous: bool = True,
        value_floor: float = 0,
    ) -> None: ...
    @property
    def fast_period(self) -> int: ...
    @property
    def slow_period(self) -> int: ...
    @property
    def value(self) -> float: ...
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
        close: float,
    ) -> None:
        """
        Update the indicator with the given raw value.

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