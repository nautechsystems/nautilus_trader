from typing import List

from nautilus_trader.model.data import Bar, QuoteTick, TradeTick

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

    name: str  # The name of the indicator
    has_inputs: bool  # If the indicator has received inputs
    initialized: bool  # If the indicator is warmed up and initialized

    def __init__(self, params: List) -> None: ...
    def __repr__(self) -> str: ...
    def handle_quote_tick(self, tick: QuoteTick) -> None: ...
    def handle_trade_tick(self, tick: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...
