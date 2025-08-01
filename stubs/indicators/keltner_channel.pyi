from nautilus_trader.indicators.average.ma_factory import MovingAverageType
from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import Indicator


class KeltnerChannel(Indicator):
    """
    The Keltner channel is a volatility based envelope set above and below a
    central moving average. Traditionally the middle band is an EMA based on the
    typical price (high + low + close) / 3, the upper band is the middle band
    plus the ATR. The lower band is the middle band minus the ATR.

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
    upper: float
    middle: float
    lower: float

    def __init__(
        self,
        period: int,
        k_multiplier: float,
        ma_type: MovingAverageType = MovingAverageType.EXPONENTIAL,
        ma_type_atr: MovingAverageType = MovingAverageType.SIMPLE,
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

    def _reset(self) -> None:
        """
        Reset the indicator.

        All stateful fields are reset to their initial value.
        """
        ...

