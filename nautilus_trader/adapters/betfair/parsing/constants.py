from betfair_parser.spec.streaming.mcm import MarketStatus as BetfairMarketStatus

from nautilus_trader.adapters.betfair.common import BETFAIR_PRICE_PRECISION
from nautilus_trader.model.enums import MarketStatus
from nautilus_trader.model.objects import Price


CLOSE_PRICE_WINNER = Price(1.0, precision=BETFAIR_PRICE_PRECISION)
CLOSE_PRICE_LOSER = Price(0.0, precision=BETFAIR_PRICE_PRECISION)


MARKET_STATUS_MAPPING: dict[tuple[BetfairMarketStatus, bool], MarketStatus] = {
    (BetfairMarketStatus.OPEN, False): MarketStatus.PRE_OPEN,
    (BetfairMarketStatus.OPEN, True): MarketStatus.OPEN,
    (BetfairMarketStatus.SUSPENDED, None): MarketStatus.PAUSE,
    (BetfairMarketStatus.CLOSED, False): MarketStatus.CLOSED,
}
