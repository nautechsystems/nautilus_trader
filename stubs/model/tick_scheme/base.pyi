from stubs.model.objects import Price

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
    def next_ask_price(self, value: float, n: int = 0) -> Price:
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
    def next_bid_price(self, value: float, n: int = 0) -> Price:
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

def round_down(value: float, base: float) -> float:
    """
    Returns a value rounded down to a specific number of decimal places.
    """
def round_up(value: float, base: float) -> float:
    """
    Returns a value rounded down to a specific number of decimal places.
    """
def register_tick_scheme(tick_scheme: TickScheme) -> None: ...
def get_tick_scheme(name: str) -> TickScheme: ...
def list_tick_schemes() -> list[str]: ...
