from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class Stochastics(Indicator):

    period_k: int
    period_d: int
    value_k: float
    value_d: float

    def __init__(self, period_k: int, period_d: int) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(self, high: float, low: float, close: float) -> None: ...
    def _reset(self) -> None: ...
