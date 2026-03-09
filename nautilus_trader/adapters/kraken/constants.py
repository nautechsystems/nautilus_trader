from typing import Final

from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue


KRAKEN: Final[str] = "KRAKEN"
KRAKEN_VENUE: Final[Venue] = Venue(KRAKEN)
KRAKEN_CLIENT_ID: Final[ClientId] = ClientId(KRAKEN)
