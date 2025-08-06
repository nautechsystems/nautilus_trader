from nautilus_trader.model.enums import PriceType
from stubs.indicators.average.moving_average import MovingAverage
from stubs.model.data import Bar
from stubs.model.data import QuoteTick
from stubs.model.data import TradeTick

class AdaptiveMovingAverage(MovingAverage):

    period_er: int
    period_alpha_fast: int
    period_alpha_slow: int
    alpha_fast: float
    alpha_slow: float
    alpha_diff: float
    value: float
    def __init__(
        self,
        period_er: int,
        period_alpha_fast: int,
        period_alpha_slow: int,
        price_type: PriceType = ...,
    ) -> None: ...
    def handle_quote_tick(self, tick: QuoteTick) -> None: ...
    def handle_trade_tick(self, tick: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(self, value: float) -> None: ...
    def _reset_ma(self) -> None: ...
