from nautilus_trader.indicators.average.moving_average import MovingAverageType
from nautilus_trader.core.nautilus_pyo3 import Bar, Price, QuoteTick, TradeTick
from nautilus_trader.core.nautilus_pyo3 import PriceType
from nautilus_trader.indicators.base.indicator import Indicator


class MovingAverageConvergenceDivergence(Indicator):
    """
    An indicator which calculates the difference between two moving averages.
    Different moving average types can be selected for the inner calculation.

    Parameters
    ----------
    fast_period : int
        The period for the fast moving average (> 0).
    slow_period : int
        The period for the slow moving average (> 0 & > fast_sma).
    ma_type : MovingAverageType
        The moving average type for the calculations.
    price_type : PriceType
        The specified price type for extracting values from quotes.

    Raises
    ------
    ValueError
        If `fast_period` is not positive (> 0).
    ValueError
        If `slow_period` is not positive (> 0).
    ValueError
        If `fast_period` is not < `slow_period`.
    """

    def __init__(
        self,
        fast_period: int,
        slow_period: int,
        ma_type: MovingAverageType = MovingAverageType.EXPONENTIAL,
        price_type: PriceType = PriceType.LAST,
    ) -> None: ...
    def handle_quote_tick(self, tick: QuoteTick) -> None:
        """
        Update the indicator with the given quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The update tick to handle.

        """
        ...
    def handle_trade_tick(self, tick: TradeTick) -> None:
        """
        Update the indicator with the given trade tick.

        Parameters
        ----------
        tick : TradeTick
            The update tick to handle.

        """
        ...
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
        Update the indicator with the given close price.

        Parameters
        ----------
        close : double
            The close price.

        """
        ...
    def _reset(self) -> None: ...
