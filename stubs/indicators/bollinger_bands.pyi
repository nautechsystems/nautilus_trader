from collections import deque

from stubs.indicators.average.moving_average import MovingAverageType
from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar
from stubs.model.data import QuoteTick
from stubs.model.data import TradeTick

class BollingerBands(Indicator):

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
    def handle_quote_tick(self, tick: QuoteTick) -> None: ...
    def handle_trade_tick(self, tick: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(self, high: float, low: float, close: float) -> None: ...
    def _reset(self) -> None: ...

