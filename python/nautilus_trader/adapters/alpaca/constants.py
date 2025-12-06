# -------------------------------------------------------------------------------------------------
#  Bot-folio Alpaca Adapter for Nautilus Trader
#  https://github.com/mandeltechnologies/bot-folio
# -------------------------------------------------------------------------------------------------

from enum import Enum

from nautilus_trader.model.identifiers import Venue


# Venue identifier
ALPACA_VENUE = Venue("ALPACA")

# REST API URLs
ALPACA_PAPER_API_URL = "https://paper-api.alpaca.markets"
ALPACA_LIVE_API_URL = "https://api.alpaca.markets"

# Data API URL (separate from trading API)
ALPACA_DATA_API_URL = "https://data.alpaca.markets"

# WebSocket URLs
ALPACA_DATA_WS_URL = "wss://stream.data.alpaca.markets/v2"
ALPACA_CRYPTO_DATA_WS_URL = "wss://stream.data.alpaca.markets/v1beta3/crypto/us"
ALPACA_PAPER_TRADING_WS_URL = "wss://paper-api.alpaca.markets/stream"
ALPACA_LIVE_TRADING_WS_URL = "wss://api.alpaca.markets/stream"


class AlpacaOrderSide(Enum):
    """Alpaca order side."""

    BUY = "buy"
    SELL = "sell"


class AlpacaOrderType(Enum):
    """Alpaca order type."""

    MARKET = "market"
    LIMIT = "limit"
    STOP = "stop"
    STOP_LIMIT = "stop_limit"
    TRAILING_STOP = "trailing_stop"


class AlpacaTimeInForce(Enum):
    """Alpaca time in force."""

    DAY = "day"
    GTC = "gtc"
    OPG = "opg"  # Market on open
    CLS = "cls"  # Market on close
    IOC = "ioc"  # Immediate or cancel
    FOK = "fok"  # Fill or kill


class AlpacaOrderStatus(Enum):
    """Alpaca order status."""

    NEW = "new"
    PARTIALLY_FILLED = "partially_filled"
    FILLED = "filled"
    DONE_FOR_DAY = "done_for_day"
    CANCELED = "canceled"
    EXPIRED = "expired"
    REPLACED = "replaced"
    PENDING_CANCEL = "pending_cancel"
    PENDING_REPLACE = "pending_replace"
    PENDING_NEW = "pending_new"
    ACCEPTED = "accepted"
    ACCEPTED_FOR_BIDDING = "accepted_for_bidding"
    STOPPED = "stopped"
    REJECTED = "rejected"
    SUSPENDED = "suspended"
    CALCULATED = "calculated"


class AlpacaAssetClass(Enum):
    """Alpaca asset class."""

    US_EQUITY = "us_equity"
    CRYPTO = "crypto"


class AlpacaAssetStatus(Enum):
    """Alpaca asset status."""

    ACTIVE = "active"
    INACTIVE = "inactive"


def get_trading_api_url(paper: bool) -> str:
    """Get the trading API base URL."""
    return ALPACA_PAPER_API_URL if paper else ALPACA_LIVE_API_URL


def get_trading_ws_url(paper: bool) -> str:
    """Get the trading WebSocket URL."""
    return ALPACA_PAPER_TRADING_WS_URL if paper else ALPACA_LIVE_TRADING_WS_URL


def get_data_ws_url(feed: str) -> str:
    """Get the data WebSocket URL for the given feed.

    Parameters
    ----------
    feed : str
        The data feed: "iex", "sip" for stocks, or "crypto" for crypto.

    """
    if feed == "crypto":
        return ALPACA_CRYPTO_DATA_WS_URL
    return f"{ALPACA_DATA_WS_URL}/{feed}"

