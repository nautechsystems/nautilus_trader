from typing import Final

from nautilus_trader.adapters.polymarket.common.enums import PolymarketTradeStatus
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue


POLYMARKET: Final[str] = "POLYMARKET"
POLYMARKET_VENUE: Final[Venue] = Venue(POLYMARKET)
POLYMARKET_CLIENT_ID: Final[ClientId] = ClientId(POLYMARKET)

POLYMARKET_MAX_PRICE: Final[float] = 0.999
POLYMARKET_MIN_PRICE: Final[float] = 0.001
POLYMARKET_MAX_PRECISION_TAKER: Final[int] = 2
POLYMARKET_MAX_PRECISION_MAKER: Final[int] = 5

VALID_POLYMARKET_TIME_IN_FORCE: Final[set[TimeInForce]] = {
    TimeInForce.GTC,
    TimeInForce.GTD,
    TimeInForce.FOK,
    TimeInForce.IOC,
}

POLYMARKET_INVALID_API_KEY: Final[str] = "Unauthorized/Invalid api key"
POLYMARKET_CANCEL_ALREADY_DONE: Final[str] = "already canceled or matched"

POLYMARKET_FINALIZED_TRADE_STATUSES: Final[tuple[PolymarketTradeStatus, ...]] = (
    PolymarketTradeStatus.MINED,
    PolymarketTradeStatus.CONFIRMED,
)

POLYMARKET_HTTP_RATE_LIMIT: Final[int] = 100  # requests per minute
