from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import CandleBodySize
from nautilus_trader.core.nautilus_pyo3 import CandleDirection
from nautilus_trader.core.nautilus_pyo3 import CandleSize
from nautilus_trader.core.nautilus_pyo3 import CandleWickSize

class FuzzyCandle:
    """
    Represents a fuzzy candle.

    Parameters
    ----------
    direction : CandleDirection
        The candle direction.
    size : CandleSize
        The candle fuzzy size.
    body_size : CandleBodySize
        The candle fuzzy body size.
    upper_wick_size : CandleWickSize
        The candle fuzzy upper wick size.
    lower_wick_size : CandleWickSize
        The candle fuzzy lower wick size.
    """

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
    """
    An indicator which fuzzifies bar data to produce fuzzy candlesticks.
    Bar data is dimensionally reduced via fuzzy feature extraction.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    threshold1 : float
        The membership function x threshold1 (>= 0).
    threshold2 : float
        The membership function x threshold2 (> threshold1).
    threshold3 : float
        The membership function x threshold3 (> threshold2).
    threshold4 : float
        The membership function x threshold4 (> threshold3).
    """

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
    def handle_bar(self, bar: Bar) -> None:
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        The update bar.

        """
        ...
    def update_raw(
        self,
        open: float,
        high: float,
        low: float,
        close: float,
    ) -> None:
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        open : double
            The open price.
        high : double
            The high price.
        low : double
            The low price.
        close : double
            The close price.

        """
        ...