from typing import Final

from betfair_parser.spec.betting import MarketStatus as BetfairMarketStatus

from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import MarketStatusAction
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price


BETFAIR: Final[str] = "BETFAIR"
BETFAIR_VENUE: Final[Venue] = Venue(BETFAIR)
BETFAIR_CLIENT_ID: Final[ClientId] = ClientId(BETFAIR)

BETFAIR_PRICE_PRECISION: Final[int] = 2
BETFAIR_QUANTITY_PRECISION: Final[int] = 2
BETFAIR_BOOK_TYPE: Final[BookType] = BookType.L2_MBP

BETFAIR_ORDER_STATUS_EXECUTABLE: Final[str] = "E"
BETFAIR_ORDER_STATUS_EXECUTION_COMPLETE: Final[str] = "EC"
BETFAIR_RATE_LIMIT_RETRY_DELAY_SECS: Final[float] = 1.0
BETFAIR_FILL_CACHE_TTL_NS: Final[int] = 900_000_000_000  # 15 minutes
BETFAIR_FILL_CACHE_SWEEP_TIMER: Final[str] = "BETFAIR_FILL_CACHE_SWEEP"

CLOSE_PRICE_WINNER: Final[Price] = Price(1.0, precision=BETFAIR_PRICE_PRECISION)
CLOSE_PRICE_LOSER: Final[Price] = Price(0.0, precision=BETFAIR_PRICE_PRECISION)

MARKET_STATUS_MAPPING: Final[dict[tuple[BetfairMarketStatus, bool], MarketStatusAction]] = {
    (BetfairMarketStatus.INACTIVE, False): MarketStatusAction.CLOSE,
    (BetfairMarketStatus.OPEN, False): MarketStatusAction.PRE_OPEN,
    (BetfairMarketStatus.OPEN, True): MarketStatusAction.TRADING,
    (BetfairMarketStatus.SUSPENDED, False): MarketStatusAction.PAUSE,
    (BetfairMarketStatus.SUSPENDED, True): MarketStatusAction.PAUSE,
    (BetfairMarketStatus.CLOSED, False): MarketStatusAction.CLOSE,
    (BetfairMarketStatus.CLOSED, True): MarketStatusAction.CLOSE,
}
