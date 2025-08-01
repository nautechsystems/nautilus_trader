from collections import deque
from typing import Any, ClassVar, Iterable

import numpy as np

from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import PriceType
from nautilus_trader.core.nautilus_pyo3 import QuoteTick
from nautilus_trader.core.nautilus_pyo3 import TradeTick
from nautilus_trader.indicators.average.nautilus_pyo3 import MovingAverage

class WeightedMovingAverage(MovingAverage):
    """
    An indicator which calculates a weighted moving average across a rolling window.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    weights : iterable
        The weights for the moving average calculation (if not ``None`` then = period).
    price_type : PriceType
        The specified price type for extracting values from quotes.

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
    """

    _inputs: deque
    weights: np.ndarray | None
    value: float
    def __init__(
        self,
        period: int,
        weights: Iterable[float] | np.ndarray | None = None,
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
            The update bar to handle.

        """
        ...
    def update_raw(self, value: float) -> None:
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        value : double
            The update value.

        """
        ...
    def _reset_ma(self) -> None: ...
