from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import QuoteTick
from nautilus_trader.core.nautilus_pyo3 import TradeTick

class Indicator:
    """
    The base class for all indicators.

    Parameters
    ----------
    params : list
        The initialization parameters for the indicator.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, params: list): ...
    def __repr__(self) -> str: ...
    def handle_quote_tick(self, tick: QuoteTick) -> None: ...
    def handle_trade_tick(self, tick: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...
