from nautilus_trader.model.enums import PriceType
from stubs.indicators.average.moving_average import MovingAverageType
from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar
from stubs.model.data import QuoteTick
from stubs.model.data import TradeTick

class MovingAverageConvergenceDivergence(Indicator):

    def __init__(
        self,
        fast_period: int,
        slow_period: int,
        ma_type: MovingAverageType = ...,
        price_type: PriceType = ...,
    ) -> None: ...
    def handle_quote_tick(self, tick: QuoteTick) -> None: ...
    def handle_trade_tick(self, tick: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(self, close: float) -> None: ...
    def _reset(self) -> None: ...
