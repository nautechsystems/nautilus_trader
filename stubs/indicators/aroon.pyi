from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class AroonOscillator(Indicator):

    period: int
    aroon_up: float
    aroon_down: float
    value: float

    def __init__(self, period: int) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(self, high: float, low: float) -> None: ...
    def _reset(self) -> None: ...
