from collections import deque
from typing import ClassVar

from nautilus_trader.core.nautilus_pyo3 import Bar, PriceType
from nautilus_trader.core.nautilus_pyo3 import EfficiencyRatio as EfficiencyRatioBase
from nautilus_trader.core.nautilus_pyo3 import Indicator


class EfficiencyRatio(Indicator):
    """
    An indicator which calculates the efficiency ratio across a rolling window.
    The Kaufman Efficiency measures the ratio of the relative market speed in
    relation to the volatility, this could be thought of as a proxy for noise.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (>= 2).

    Raises
    ------
    ValueError
        If `period` is not >= 2.
    """

    period: int
    value: float
    _inputs: ClassVar[deque]
    _deltas: ClassVar[deque]

    def __init__(self, period: int) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(self, price: float) -> None: ...