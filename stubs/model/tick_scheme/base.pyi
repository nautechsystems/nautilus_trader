from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Condition

class TickScheme:
    """
    Represents an instrument tick scheme.

    Maps the valid prices available for an instrument.

    Parameters
    ----------
    name : str
        The name of the tick scheme.
    min_tick : Price
        The minimum possible tick `Price`.
    max_tick: Price
        The maximum possible tick `Price`.

    Raises
    ------
    ValueError
        If `name` is not a valid string.
    """

    def __init__(
        self,
        name: str,
        min_tick: Price,
        max_tick: Price,
    ) -> None: ...
    def next_ask_price(self, value: float, n: int = 0) -> Price: ...
    def next_bid_price(self, value: float, n: int = 0) -> Price: ...

def round_down(value: float, base: float) -> float: ...
def round_up(value: float, base: float) -> float: ...
def register_tick_scheme(tick_scheme: TickScheme) -> None: ...
def get_tick_scheme(name: str) -> TickScheme: ...
def list_tick_schemes() -> list[str]: ...