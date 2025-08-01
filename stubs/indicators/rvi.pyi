from nautilus_trader.indicators.average.ma_factory import MovingAverageType
from nautilus_trader.model.data import Bar
from nautilus_trader.indicators.base.indicator import Indicator


class RelativeVolatilityIndex(Indicator):
    """
    The Relative Volatility Index (RVI) was created in 1993 and revised in 1995.
    Instead of adding up price changes like RSI based on price direction, the RVI
    adds up standard deviations based on price direction.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    scalar : double
        A positive float to scale the bands.
    ma_type : MovingAverageType
        The moving average type for the vip and vim (cannot be None).
    """

    period: int
    scalar: float
    value: float

    def __init__(
        self,
        period: int,
        scalar: float = 100.0,
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
    def update_raw(self, close: float) -> None:
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
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
