from stubs.indicators.average.moving_average import MovingAverageType
from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class AverageTrueRange(Indicator):
    """
    An indicator which calculates the average true range across a rolling window.
    Different moving average types can be selected for the inner calculation.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    ma_type : MovingAverageType
        The moving average type for the indicator (cannot be None).
    use_previous : bool
        The boolean flag indicating whether previous price values should be used.
        (note: only applicable for `update()`. `update_mid()` will need to
        use previous price.
    value_floor : double
        The floor (minimum) output value for the indicator (>= 0).
    """

    period: int
    value: float

    def __init__(
        self,
        period: int,
        ma_type: MovingAverageType = ...,
        use_previous: bool = True,
        value_floor: float = 0,
    ) -> None: ...
    def handle_bar(self, bar: Bar) -> None:
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar.

        """
    def update_raw(
        self,
        high: float,
        low: float,
        close: float,
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

        """
    def _reset(self) -> None: ...
