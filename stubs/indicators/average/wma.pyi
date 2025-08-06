from collections import deque
from collections.abc import Iterable

import numpy as np

from nautilus_trader.model.enums import PriceType
from stubs.indicators.average.moving_average import MovingAverage
from stubs.model.data import Bar
from stubs.model.data import QuoteTick
from stubs.model.data import TradeTick

class WeightedMovingAverage(MovingAverage):

    _inputs: deque
    weights: np.ndarray | None
    value: float
    def __init__(
        self,
        period: int,
        weights: Iterable[float] | np.ndarray | None = None,
        price_type: PriceType = ...,
    ) -> None: ...
    def handle_quote_tick(self, tick: QuoteTick) -> None: ...
    def handle_trade_tick(self, tick: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(self, value: float) -> None: ...
    def _reset_ma(self) -> None: ...
