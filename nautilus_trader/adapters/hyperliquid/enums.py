from __future__ import annotations

from enum import Enum
from enum import unique


@unique
class HyperliquidProductType(str, Enum):
    """
    Supported Hyperliquid product types for instrument discovery.
    """

    SPOT = "spot"
    PERP = "perp"

    @property
    def is_spot(self) -> bool:
        return self is HyperliquidProductType.SPOT

    @property
    def is_perp(self) -> bool:
        return self is HyperliquidProductType.PERP


DEFAULT_PRODUCT_TYPES = frozenset({HyperliquidProductType.SPOT, HyperliquidProductType.PERP})
