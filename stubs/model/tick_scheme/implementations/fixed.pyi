from stubs.model.objects import Price
from stubs.model.tick_scheme.base import TickScheme

class FixedTickScheme(TickScheme):

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
    def next_ask_price(self, value: float, n: int = 0) -> Price | None: ...
    def next_bid_price(self, value: float, n: int = 0) -> Price | None: ...

FOREX_5DECIMAL_TICK_SCHEME: FixedTickScheme
FOREX_3DECIMAL_TICK_SCHEME: FixedTickScheme
