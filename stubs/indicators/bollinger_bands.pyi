from collections import deque

from stubs.indicators.average.moving_average import MovingAverageType
from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar
from stubs.model.data import QuoteTick
from stubs.model.data import TradeTick

class BollingerBands(Indicator):
    """
    A Bollinger Band® is a technical analysis tool defined by a set of
    trend lines plotted two standard deviations (positively and negatively) away
    from a simple moving average (SMA) of an instruments price, which can be
    adjusted to user preferences.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    k : double
        The standard deviation multiple for the indicator (> 0).
    ma_type : MovingAverageType
        The moving average type for the indicator.

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
    ValueError
        If `k` is not positive (> 0).
    """

    period: int
    k: float
    upper: float
    middle: float
    lower: float
    _ma: MovingAverageType
    _prices: deque

    def __init__(
        self,
        period: int,
        k: float,
        ma_type: MovingAverageType = ...,
    ) -> None: ...
    def handle_quote_tick(self, tick: QuoteTick) -> None:
        """
        Update the indicator with the given tick.

        Parameters
        ----------
        tick : TradeTick
            The tick for the update.

        """
    def handle_trade_tick(self, tick: TradeTick) -> None:
        """
        Update the indicator with the given tick.

        Parameters
        ----------
        tick : TradeTick
            The tick for the update.

        """
    def handle_bar(self, bar: Bar) -> None:
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar.

        """
    def update_raw(self, high: float, low: float, close: float) -> None:
        """
        Update the indicator with the given prices.

        Parameters
        ----------
        high : double
            The high price for calculations.
        low : double
            The low price for calculations.
        close : double
            The closing price for calculations

        """
    def _reset(self) -> None: ...

