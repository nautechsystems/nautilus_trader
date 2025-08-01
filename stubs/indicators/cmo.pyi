from stubs.indicators.average.moving_average import MovingAverage
from stubs.indicators.base.indicator import Indicator

class ChandeMomentumOscillator(Indicator):
    """
    Attempts to capture the momentum of an asset with overbought at 50 and
    oversold at -50.

    Parameters
    ----------
    ma_type : int
        The moving average type for average gain/loss.
    period : MovingAverageType
        The rolling window period for the indicator.
    """

    period: int
    value: float
    _average_gain: MovingAverage
    _average_loss: MovingAverage
    _previous_close: float

    def __init__(
        self,
        period: int,
        ma_type: MovingAverageType = MovingAverageType.WILDER,
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
    def update_raw(self, close: float) -> None:
        """
        Update the indicator with the given value.

        Parameters
        ----------
        value : double
            The update value.

        """
        ...
    def _reset(self) -> None: ...
