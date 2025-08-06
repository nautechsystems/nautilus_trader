from stubs.indicators.atr import AverageTrueRange
from stubs.indicators.average.ma_factory import MovingAverageFactory
from stubs.indicators.average.moving_average import MovingAverageType
from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class Pressure(Indicator):
    """
    An indicator which calculates the relative volume (multiple of average volume)
    to move the market across a relative range (multiple of ATR).

    Parameters
    ----------
    period : int
        The period for the indicator (> 0).
    ma_type : MovingAverageType
        The moving average type for the calculations.
    atr_floor : double
        The ATR floor (minimum) output value for the indicator (>= 0.).

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
    ValueError
        If `atr_floor` is negative (< 0).
    """

    period: int
    value: float
    value_cumulative: float
    _atr: AverageTrueRange
    _average_volume: MovingAverageFactory

    def __init__(
        self,
        period: int,
        ma_type: MovingAverageType = ...,
        atr_floor: float = 0,
    ) -> None: ...
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
        volume: float,
    ) -> None:
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
        volume : double
            The volume.

        """
        ...
    def _reset(self) -> None: ...
