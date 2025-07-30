from nautilus_trader.indicators.average.ma_factory import MovingAverageType
from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import RelativeVolatilityIndex as Indicator # Importing from symbol reference since parent class is skipped


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

    def __init__(
        self,
        period: int,
        scalar: float = 100.0,
        ma_type: MovingAverageType = MovingAverageType.EXPONENTIAL,
    ) -> None: ...
    @property
    def period(self) -> int: ...
    @property
    def scalar(self) -> float: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
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
    def update_raw(self, close: float) -> None:
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        close : double
            The close price.

        """
        ...