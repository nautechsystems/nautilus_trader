from typing import Final

from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue


BITMEX: Final[str] = "BITMEX"
BITMEX_VENUE: Final[Venue] = Venue(BITMEX)
BITMEX_CLIENT_ID: Final[ClientId] = ClientId(BITMEX)
