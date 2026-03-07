# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from enum import Enum
from typing import Final

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue


BITGET: Final[str] = "BITGET"
BITGET_VENUE: Final[Venue] = Venue(BITGET)
BITGET_CLIENT_ID: Final[ClientId] = ClientId(BITGET)

_bitget_product_type = getattr(nautilus_pyo3, "BitgetProductType", None)

if _bitget_product_type is None:

    class _FallbackBitgetProductType(Enum):
        SPOT = "SPOT"
        USDT_FUTURES = "USDT-FUTURES"
        COIN_FUTURES = "COIN-FUTURES"
        USDC_FUTURES = "USDC-FUTURES"

    _bitget_product_type = _FallbackBitgetProductType

BITGET_DEFAULT_PRODUCTS: Final[tuple[object, ...]] = (
    _bitget_product_type.SPOT,
    _bitget_product_type.USDT_FUTURES,
    _bitget_product_type.COIN_FUTURES,
    _bitget_product_type.USDC_FUTURES,
)
