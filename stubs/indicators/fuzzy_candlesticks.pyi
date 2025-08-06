from nautilus_trader.core.nautilus_pyo3 import CandleBodySize
from nautilus_trader.core.nautilus_pyo3 import CandleDirection
from nautilus_trader.core.nautilus_pyo3 import CandleSize
from nautilus_trader.core.nautilus_pyo3 import CandleWickSize
from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class FuzzyCandle:

    direction: CandleDirection
    size: CandleSize
    body_size: CandleBodySize
    upper_wick_size: CandleWickSize
    lower_wick_size: CandleWickSize

    def __init__(
        self,
        direction: CandleDirection,
        size: CandleSize,
        body_size: CandleBodySize,
        upper_wick_size: CandleWickSize,
        lower_wick_size: CandleWickSize,
    ) -> None: ...
    def __eq__(self, other: FuzzyCandle) -> bool: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class FuzzyCandlesticks(Indicator):

    period: int
    vector: list[int] | None
    value: FuzzyCandle | None

    def __init__(
        self,
        period: int,
        threshold1: float = 0.5,
        threshold2: float = 1.0,
        threshold3: float = 2.0,
        threshold4: float = 3.0,
    ) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(
        self,
        open: float,
        high: float,
        low: float,
        close: float,
    ) -> None: ...
    def _reset(self) -> None: ...
