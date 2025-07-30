from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import Indicator

class AroonOscillator(Indicator):
    """
    The Aroon (AR) indicator developed by Tushar Chande attempts to
    determine whether an instrument is trending, and how strong the trend is.
    AroonUp and AroonDown lines make up the indicator with their formulas below.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    """

    period: int
    aroon_up: float
    aroon_down: float
    value: float

    def __init__(self, period: int) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(self, high: float, low: float) -> None: ...