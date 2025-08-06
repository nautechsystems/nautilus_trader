import numpy as np

from stubs.model.objects import Price
from stubs.model.tick_scheme.base import TickScheme

class TieredTickScheme(TickScheme):

    price_precision: int
    tiers: list[tuple[float, float, float]]
    max_ticks_per_tier: int
    ticks: np.ndarray
    tick_count: int

    def __init__(
        self,
        name: str,
        tiers: list[tuple[float, float, float]],
        price_precision: int,
        max_ticks_per_tier: int = 100,
    ) -> None: ...
    def _build_ticks(self) -> np.ndarray: ...
    def find_tick_index(self, value: float) -> int: ...
    def next_ask_price(self, value: float, n: int = 0) -> Price: ...
    def next_bid_price(self, value: float, n: int = 0) -> Price: ...

    @staticmethod
    def _validate_tiers(tiers: list) -> list: ...

TOPIX100_TICK_SCHEME: TieredTickScheme
