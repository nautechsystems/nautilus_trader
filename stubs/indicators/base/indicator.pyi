from stubs.model.data import Bar
from stubs.model.data import QuoteTick
from stubs.model.data import TradeTick

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
    def handle_quote_tick(self, tick: QuoteTick) -> None:
        """Abstract method (implement in subclass)."""
        ...
    def handle_trade_tick(self, tick: TradeTick) -> None:
        """Abstract method (implement in subclass)."""
        ...
    def handle_bar(self, bar: Bar) -> None:
        """Abstract method (implement in subclass)."""
        ...
    def reset(self) -> None:
        """
        Reset the indicator.

        All stateful fields are reset to their initial value.
        """
        ...
    def _set_has_inputs(self, setting: bool) -> None: ...
    def _set_initialized(self, setting: bool) -> None: ...
    def _reset(self) -> None:
        """Abstract method (implement in subclass)."""
        ...
