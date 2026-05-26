# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter
#  Licensed under the GNU Lesser General Public License Version 3.0
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.identifiers import Venue


# Venue identifier — used as the suffix in all InstrumentId values (e.g. BTC-USD.LMEX)
LMEX_VENUE: Venue = Venue("LMEX")

# REST base URLs
LMEX_BASE_URL_LIVE: str = "https://api.lmex.io/spot"
LMEX_BASE_URL_SANDBOX: str = "https://test-api.lmex.io/spot"

# WebSocket URLs
# Note: the live URL path is /ws/spot (not /spot/ws as stated in some docs)
LMEX_WS_URL_LIVE: str = "wss://ws.lmex.io/ws/spot"
LMEX_WS_URL_SANDBOX: str = "wss://ws.test-api.lmex.io/ws/spot"

# WebSocket heartbeat interval in seconds
LMEX_WS_HEARTBEAT_SECS: int = 10

# Default HTTP request timeout in seconds
LMEX_HTTP_TIMEOUT_SECS: int = 10

# WebSocket topic names
LMEX_WS_TOPIC_TRADES: str = "tradeHistoryApi"
LMEX_WS_TOPIC_ORDERBOOK: str = "orderBookApi"
LMEX_WS_TOPIC_NOTIFICATIONS: str = "notificationsApi"

# Environment variable names for credentials
LMEX_API_KEY_ENV: str = "LMEX_API_KEY"
LMEX_API_SECRET_ENV: str = "LMEX_API_SECRET"
LMEX_SANDBOX_API_KEY_ENV: str = "LMEX_SANDBOX_API_KEY"
LMEX_SANDBOX_API_SECRET_ENV: str = "LMEX_SANDBOX_API_SECRET"
