from nautilus_trader.model.enums import PriceType
from stubs.indicators.average.moving_average import MovingAverage
from stubs.indicators.average.moving_average import MovingAverageType
from stubs.indicators.cmo import ChandeMomentumOscillator
from stubs.model.data import Bar
from stubs.model.data import QuoteTick
from stubs.model.data import TradeTick

class VariableIndexDynamicAverage(MovingAverage):

    cmo: ChandeMomentumOscillator
    cmo_pct: float
    alpha: float
    value: float

    def __init__(
        self,
        period: int,
        price_type: PriceType = ...,
        cmo_ma_type: MovingAverageType = ...,
    ) -> None: ...
    def handle_quote_tick(self, tick: QuoteTick) -> None: ...
    def handle_trade_tick(self, tick: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(self, value: float) -> None: ...
