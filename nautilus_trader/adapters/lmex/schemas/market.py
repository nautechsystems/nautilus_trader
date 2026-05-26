# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter
#  Licensed under the GNU Lesser General Public License Version 3.0
# -------------------------------------------------------------------------------------------------

"""
msgspec Struct definitions for LMEX REST market-data responses.

All timestamps from LMEX are in **epoch milliseconds**. Conversion to
nanoseconds (required by NautilusTrader) is done in the parsing layer,
not here.
"""

from __future__ import annotations

import msgspec


# ---------------------------------------------------------------------------
# Server time
# ---------------------------------------------------------------------------


class LmexServerTime(msgspec.Struct, frozen=True):
    """
    Response schema for ``GET /api/v3.2/time``.

    Attributes
    ----------
    iso : str
        ISO-8601 formatted server time.
    epoch : int
        Unix timestamp in **seconds** (not milliseconds).

    """

    iso: str
    epoch: int


# ---------------------------------------------------------------------------
# Order book
# ---------------------------------------------------------------------------


class LmexOrderBookEntry(msgspec.Struct, frozen=True):
    """
    A single price level in the LMEX order book.

    Attributes
    ----------
    price : str
        Price as a decimal string (e.g. ``"76668.1"``).
    size : str
        Quantity as a decimal string (e.g. ``"0.04350"``).

    Notes
    -----
    Prices and sizes are returned as *strings* in the REST response
    (unlike the WebSocket feed, which uses floats).

    """

    price: str
    size: str


class LmexOrderBook(msgspec.Struct, frozen=True):
    """
    Response schema for ``GET /api/v3.2/orderbook``.

    Attributes
    ----------
    symbol : str
        The trading pair symbol (e.g. ``"BTC-USD"``).
    buyQuote : list[LmexOrderBookEntry]
        Bid levels sorted best-to-worst (highest price first).
    sellQuote : list[LmexOrderBookEntry]
        Ask levels sorted best-to-worst (lowest price first).

    """

    symbol: str
    buyQuote: list[LmexOrderBookEntry]
    sellQuote: list[LmexOrderBookEntry]


# ---------------------------------------------------------------------------
# Trades
# ---------------------------------------------------------------------------


class LmexTrade(msgspec.Struct, frozen=True):
    """
    A single public trade from ``GET /api/v3.2/trades``.

    Attributes
    ----------
    price : float
        Execution price.
    size : float
        Executed quantity.
    side : str
        ``"BUY"`` or ``"SELL"`` (taker side).
    symbol : str
        Trading pair (e.g. ``"BTC-USD"``).
    serialId : int
        Unique sequential trade identifier used in REST responses.
    timestamp : int
        Unix timestamp in **milliseconds**.

    """

    price: float
    size: float
    side: str
    symbol: str
    serialId: int
    timestamp: int


# ---------------------------------------------------------------------------
# Market summary / instrument info
# ---------------------------------------------------------------------------


class LmexMarketSummary(msgspec.Struct):
    """
    One entry from ``GET /api/v3.2/market_summary``.

    This is the primary source for constructing NautilusTrader
    instrument definitions (``CurrencyPair``).

    Attributes
    ----------
    symbol : str
        Trading pair (e.g. ``"BTC-USD"``).
    base : str
        Base asset code (e.g. ``"BTC"``).
    quote : str
        Quote asset code (e.g. ``"USD"``).
    last : float
        Last trade price.
    lowestAsk : float
        Current best ask.
    highestBid : float
        Current best bid.
    minValidPrice : float
        Minimum valid price tick.
    minPriceIncrement : float
        Minimum price increment (tick size).
    minOrderSize : float
        Minimum order quantity.
    maxOrderSize : float
        Maximum order quantity.
    minSizeIncrement : float
        Minimum quantity increment (lot size).
    active : bool
        Whether the market is currently open for trading.
    futures : bool
        ``True`` for perpetual / futures instruments.
    isMarketOpenToSpot : bool
        ``True`` when spot trading is enabled.

    """

    symbol: str
    base: str
    quote: str
    last: float
    lowestAsk: float
    highestBid: float
    minValidPrice: float
    minPriceIncrement: float
    minOrderSize: float
    maxOrderSize: float
    minSizeIncrement: float
    active: bool
    futures: bool
    isMarketOpenToSpot: bool
    # Optional fields that may be absent on older API versions
    percentageChange: float | None = None
    volume: float | None = None
    high24Hr: float | None = None
    low24Hr: float | None = None
    size: float | None = None
    openInterest: float | None = None
    openInterestUSD: float | None = None
    contractStart: int | None = None
    contractEnd: int | None = None
    timeBasedContract: bool | None = None
    openTime: int | None = None
    closeTime: int | None = None
    startMatching: int | None = None
    inactiveTime: int | None = None
    fundingRate: float | None = None
    contractSize: float | None = None
    maxPosition: float | None = None
    minRiskLimit: float | None = None
    maxRiskLimit: float | None = None
    availableSettlement: list[str] | None = None
    isMarketOpenToOtc: bool | None = None
