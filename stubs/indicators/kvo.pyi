from nautilus_trader.indicators.average.moving_average import MovingAverageType
from nautilus_trader.core.nautilus_pyo3 import Bar, MovingAverageType
from nautilus_trader.core.nautilus_pyo3 import KlingerVolumeOscillator as Indicator # Added for inheritance

class KlingerVolumeOscillator(Indicator):
    """
    This indicator was developed by Stephen J. Klinger. It is designed to predict
    price reversals in a market by comparing volume to price.

    Parameters
    ----------
    fast_period : int
        The period for the fast moving average (> 0).
    slow_period : int
        The period for the slow moving average (> 0 & > fast_sma).
    signal_period : int
        The period for the moving average difference's moving average (> 0).
    ma_type : MovingAverageType
        The moving average type for the calculations.
    """

    def __init__(
        self,
        fast_period: int,
        slow_period: int,
        signal_period: int,
        ma_type: MovingAverageType = MovingAverageType.EXPONENTIAL,
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
    def reset(self) -> None: ...