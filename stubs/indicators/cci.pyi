from collections import deque
from nautilus_trader.indicators.average.ma_factory import MovingAverageType
from nautilus_trader.indicators.average.ma_factory import MovingAverageFactory
from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import MovingAverageType
from nautilus_trader.core.nautilus_pyo3 import Indicator
import numpy as np


class CommodityChannelIndex(Indicator):
    """
    Commodity Channel Index is a momentum oscillator used to primarily identify
    overbought and oversold levels relative to a mean.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    scalar : double
        A positive float to scale the bands
    ma_type : MovingAverageType
        The moving average type for prices.

    References
    ----------
    https://www.tradingview.com/support/solutions/43000502001-commodity-channel-index-cci/
    """

    def __init__(
        self,
        period: int,
        scalar: float = 0.015,
        ma_type: MovingAverageType = MovingAverageType.SIMPLE,
    ) -> None: ...
    @property
    def name(self) -> str: ...
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
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(
        self,
        high: float,
        low: float,
        close: float,
    ) -> None: ...
    def reset(self) -> None: ...