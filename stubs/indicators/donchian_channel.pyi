from collections import deque

from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar
from stubs.model.data import QuoteTick
from stubs.model.data import TradeTick

class DonchianChannel(Indicator):

    period: int
    upper: float
    middle: float
    lower: float

    _upper_prices: deque
    _lower_prices: deque

    def __init__(self, period: int) -> None: ...
    def handle_quote_tick(self, tick: QuoteTick) -> None: ...
    def handle_trade_tick(self, tick: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(self, high: float, low: float) -> None: ...
    def _reset(self) -> None: ...
