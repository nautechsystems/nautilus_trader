from __future__ import annotations

from typing import Final

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue


HYPERLIQUID: Final[str] = "HYPERLIQUID"
HYPERLIQUID_VENUE: Final[Venue] = Venue(HYPERLIQUID)
HYPERLIQUID_CLIENT_ID: Final[ClientId] = ClientId(HYPERLIQUID)

# Error message substrings for detecting specific rejection reasons
HYPERLIQUID_POST_ONLY_WOULD_MATCH: Final[str] = nautilus_pyo3.HYPERLIQUID_POST_ONLY_WOULD_MATCH
