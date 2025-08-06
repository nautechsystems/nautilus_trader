from stubs.indicators.average.moving_average import MovingAverageType
from stubs.indicators.base.indicator import Indicator
from stubs.indicators.keltner_channel import KeltnerChannel
from stubs.model.data import Bar

class KeltnerPosition(Indicator):
    """
    An indicator which calculates the relative position of the given price
    within a defined Keltner channel. This provides a measure of the relative
    'extension' of a market from the mean, as a multiple of volatility.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    k_multiplier : double
        The multiplier for the ATR (> 0).
    ma_type : MovingAverageType
        The moving average type for the middle band (cannot be None).
    ma_type_atr : MovingAverageType
        The moving average type for the internal ATR (cannot be None).
    use_previous : bool
        The boolean flag indicating whether previous price values should be used.
    atr_floor : double
        The ATR floor (minimum) output value for the indicator (>= 0).
    """

    period: int
    k_multiplier: float
    value: float
    _kc: KeltnerChannel

    def __init__(
        self,
        period: int,
        k_multiplier: float,
        ma_type: MovingAverageType = ...,
        ma_type_atr: MovingAverageType = ...,
        use_previous: bool = True,
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
    def _reset(self) -> None: ...
