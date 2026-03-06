from typing import Final

from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue


OKX: Final[str] = "OKX"
OKX_VENUE: Final[Venue] = Venue(OKX)
OKX_CLIENT_ID: Final[ClientId] = ClientId(OKX)
