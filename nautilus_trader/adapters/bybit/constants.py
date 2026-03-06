from typing import Final

from nautilus_trader.core.nautilus_pyo3 import BybitProductType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue


BYBIT: Final[str] = "BYBIT"
BYBIT_VENUE: Final[Venue] = Venue(BYBIT)
BYBIT_CLIENT_ID: Final[ClientId] = ClientId(BYBIT)

BYBIT_ALL_PRODUCTS: Final[tuple[BybitProductType, ...]] = (
    BybitProductType.SPOT,
    BybitProductType.LINEAR,
    BybitProductType.INVERSE,
    BybitProductType.OPTION,
)

BYBIT_MULTIPLIERS: Final[list[int]] = [1000000, 100000, 10000, 1000, 100, 10]
