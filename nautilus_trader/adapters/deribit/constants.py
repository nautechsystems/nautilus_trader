from typing import Final

from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue


DERIBIT: Final[str] = "DERIBIT"
DERIBIT_VENUE: Final[Venue] = Venue(DERIBIT)
DERIBIT_CLIENT_ID: Final[ClientId] = ClientId(DERIBIT)

# WebSocket session names for authentication
DERIBIT_DATA_SESSION_NAME: Final[str] = "nautilus-data"
DERIBIT_EXECUTION_SESSION_NAME: Final[str] = "nautilus-execution"

# WebSocket heartbeat interval in seconds (Deribit recommends 30-60s)
DERIBIT_WS_HEARTBEAT_SECS: Final[int] = 30
