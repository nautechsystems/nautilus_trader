from typing import ClassVar
from nautilus_trader.model.objects import Price
from nautilus_trader.model.tick_scheme.base import TickScheme

class FixedTickScheme(TickScheme):
    """
    Represents a fixed precision tick scheme such as for Forex or Crypto.

    Parameters
    ----------
    name : str
        The name of the tick scheme.
    price_precision: int
        The instrument price precision.
    min_tick : Price
        The minimum possible tick `Price`.
    max_tick: Price
        The maximum possible tick `Price`.
    increment : float, optional
        The tick increment.

    Raises
    ------
    ValueError
        If `name` is not a valid string.
    """

    price_precision: int
    increment: Price

    def __init__(
        self,
        name: str,
        price_precision: int,
        min_tick: Price,
        max_tick: Price,
        increment: float | None = None,
    ) -> None: ...
    def next_ask_price(self, value: float, n: int = 0) -> Price | None:
        """
        Return the price `n` ask ticks away from value.

        If a given price is between two ticks, n=0 will find the nearest ask tick.

        Parameters
        ----------
        value : double
            The reference value.
        n : int, default 0
            The number of ticks to move.

        Returns
        -------
        Price

        """
    def next_bid_price(self, value: float, n: int = 0) -> Price | None:
        """
        Return the price `n` bid ticks away from value.

        If a given price is between two ticks, n=0 will find the nearest bid tick.

        Parameters
        ----------
        value : double
            The reference value.
        n : int, default 0
            The number of ticks to move.

        Returns
        -------
        Price

        """

FOREX_5DECIMAL_TICK_SCHEME: FixedTickScheme
FOREX_3DECIMAL_TICK_SCHEME: FixedTickScheme
