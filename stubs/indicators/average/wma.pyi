from collections import deque
from collections.abc import Iterable

import numpy as np

from nautilus_trader.model.enums import PriceType
from stubs.indicators.average.moving_average import MovingAverage
from stubs.model.data import Bar
from stubs.model.data import QuoteTick
from stubs.model.data import TradeTick

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
        price_type: PriceType = ...,
    ) -> None: ...
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
