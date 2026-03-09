from typing import Final

from nautilus_trader.adapters.polymarket.schemas.book import PolymarketBookSnapshot
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketQuotes
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketTickSizeChange
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketTrade
from nautilus_trader.adapters.polymarket.schemas.user import PolymarketUserOrder
from nautilus_trader.adapters.polymarket.schemas.user import PolymarketUserTrade


MARKET_WS_MESSAGE: Final = (
    list[PolymarketBookSnapshot]
    | PolymarketBookSnapshot
    | PolymarketQuotes
    | PolymarketTrade
    | PolymarketTickSizeChange
)
USER_WS_MESSAGE: Final = PolymarketUserOrder | PolymarketUserTrade
