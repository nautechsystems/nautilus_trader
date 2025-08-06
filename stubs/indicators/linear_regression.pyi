from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class LinearRegression(Indicator):

    period: int
    slope: float
    intercept: float
    degree: float
    cfo: float
    R2: float
    value: float

    def __init__(self, period: int = 0) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(self, close: float) -> None: ...
    def _reset(self) -> None: ...
