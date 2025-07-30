from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import MovingAverageType
from nautilus_trader.core.nautilus_pyo3 import PriceType
from nautilus_trader.core.nautilus_pyo3 import QuoteTick
from nautilus_trader.core.nautilus_pyo3 import TradeTick
from typing import ClassVar


class WilderMovingAverage(MovingAverage):
    """
    The Wilder's Moving Average is simply an Exponential Moving Average (EMA) with
    a modified alpha = 1 / period.

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

    period: ClassVar[int]
    alpha: ClassVar[float]
    value: ClassVar[float]
    def __init__(self, period: int, price_type: PriceType = PriceType.LAST) -> None: ...
    def handle_quote_tick(self, tick: QuoteTick) -> None: ...
    def handle_trade_tick(self, tick: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(self, value: float) -> None: ...