from nautilus_trader.model.enums import PriceType
from stubs.indicators.average.ema import ExponentialMovingAverage
from stubs.indicators.average.moving_average import MovingAverage
from stubs.model.data import Bar
from stubs.model.data import QuoteTick
from stubs.model.data import TradeTick

class DoubleExponentialMovingAverage(MovingAverage):
    """
    The Double Exponential Moving Average attempts to a smoother average with less
    lag than the normal Exponential Moving Average (EMA).

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    price_type : PriceType
        The specified price type for extracting values from quotes.

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
    """

    value: float
    _ma1: ExponentialMovingAverage
    _ma2: ExponentialMovingAverage

    def __init__(self, period: int, price_type: PriceType = ...) -> None: ...
    def handle_quote_tick(self, tick: QuoteTick) -> None:
        """
        Update the indicator with the given quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The update tick to handle.

        """
    def handle_trade_tick(self, tick: TradeTick) -> None:
        """
        Update the indicator with the given trade tick.

        Parameters
        ----------
        tick : TradeTick
            The update tick to handle.

        """
    def handle_bar(self, bar: Bar) -> None:
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar to handle.

        """
    def update_raw(self, value: float) -> None:
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        value : double
            The update value.

        """
    def _reset_ma(self) -> None: ...

