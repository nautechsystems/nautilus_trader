from typing import Final

from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue


AX: Final[str] = "AX"
AX_VENUE: Final[Venue] = Venue(AX)
AX_CLIENT_ID: Final[ClientId] = ClientId(AX)

AX_SUPPORTED_ORDER_TYPES = (OrderType.MARKET, OrderType.LIMIT, OrderType.STOP_LIMIT)

AX_WS_ORDERS_SANDBOX_URL = "wss://gateway.sandbox.architect.exchange/orders/ws"
AX_WS_ORDERS_PRODUCTION_URL = "wss://gateway.architect.exchange/orders/ws"
