from collections import deque
from nautilus_trader.core.nautilus_pyo3 import Bar, MovingAverageType, PriceType, QuoteTick, TradeTick
from nautilus_trader.indicators.base.indicator import Indicator

class Stochastics(Indicator):
    """
    An oscillator which can indicate when an asset may be over bought or over
    sold.

    Parameters
    ----------
    period_k : int
        The period for the K line.
    period_d : int
        The period for the D line.

    Raises
    ------
    ValueError
        If `period_k` is not positive (> 0).
    ValueError
        If `period_d` is not positive (> 0).

    References
    ----------
    https://www.forextraders.com/forex-education/forex-indicators/stochastics-indicator-explained/
    """

    def __init__(self, period_k: int, period_d: int) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period_k(self) -> int: ...
    @property
    def period_d(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value_k(self) -> float: ...
    @property
    def value_d(self) -> float: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(self, high: float, low: float, close: float) -> None: ...
    def reset(self) -> None: ...