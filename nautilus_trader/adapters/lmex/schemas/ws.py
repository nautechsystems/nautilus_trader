# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter
#  Licensed under the GNU Lesser General Public License Version 3.0
# -------------------------------------------------------------------------------------------------

"""
msgspec Struct definitions for LMEX WebSocket message envelopes.

Confirmed live structure (probed 2026-05-26 against wss://ws.lmex.io/ws/spot):

  Subscription ack:
    {"event": "subscribe", "channel": ["tradeHistoryApi:BTC-USD"]}

  Trade feed (topic = "tradeHistoryApi:<symbol>"):
    {"topic": "tradeHistoryApi:BTC-USD",
     "data": [{"symbol", "side", "size", "price", "tradeId", "timestamp"}]}

WebSocket message types are dispatched by the ``topic`` field when present,
or by the ``event`` field for control messages (subscribe ack, pong, etc.).
"""

from __future__ import annotations

import msgspec


# ---------------------------------------------------------------------------
# Control / ack messages
# ---------------------------------------------------------------------------


class LmexWsSubscribeAck(msgspec.Struct):
    """
    Server acknowledgement for a subscription or unsubscription request.

    Attributes
    ----------
    event : str
        ``"subscribe"`` or ``"unsubscribe"``.
    channel : list[str]
        Topics that were successfully subscribed/unsubscribed.
        Empty list if the subscription was rejected.

    """

    event: str
    channel: list[str]


class LmexWsPongMsg(msgspec.Struct):
    """
    Server heartbeat pong response.

    Attributes
    ----------
    event : str
        ``"pong"``.

    """

    event: str


# ---------------------------------------------------------------------------
# Trade feed
# ---------------------------------------------------------------------------


class LmexWsTradeDatum(msgspec.Struct, frozen=True):
    """
    A single trade record within a ``tradeHistoryApi`` WebSocket message.

    Attributes
    ----------
    symbol : str
        Trading pair (e.g. ``"BTC-USD"``).
    side : str
        ``"BUY"`` or ``"SELL"`` (taker aggressor side).
    size : float
        Executed quantity.
    price : float
        Execution price.
    tradeId : int
        Unique sequential trade identifier.
    timestamp : int
        Unix timestamp in **milliseconds**.

    Notes
    -----
    Unlike the REST trade endpoint (which uses ``serialId``), the WebSocket
    uses ``tradeId`` for the trade identifier.

    """

    symbol: str
    side: str
    size: float
    price: float
    tradeId: int
    timestamp: int


class LmexWsTradeMsg(msgspec.Struct, frozen=True):
    """
    WebSocket message envelope for trade feed updates.

    Attributes
    ----------
    topic : str
        Topic string (e.g. ``"tradeHistoryApi:BTC-USD"``).
    data : list[LmexWsTradeDatum]
        One or more trade records.  Initial snapshot may contain many entries;
        subsequent updates typically contain one entry.

    """

    topic: str
    data: list[LmexWsTradeDatum]


# ---------------------------------------------------------------------------
# Order book feed
# ---------------------------------------------------------------------------
# Note: The orderbook WebSocket topic name (e.g. orderBookApi:BTC-USD_0) is
# to be confirmed during implementation with sandbox credentials.
# The struct below is designed to match the REST orderbook shape, which is
# the most likely WS format based on exchange convention.


class LmexWsOrderBookEntry(msgspec.Struct, frozen=True):
    """
    A single price level in a WebSocket order book update.

    Attributes
    ----------
    price : float
        Price level.
    size : float
        Quantity at this price level. ``0.0`` means the level was removed.

    """

    price: float
    size: float


class LmexWsOrderBookData(msgspec.Struct, frozen=True):
    """
    Data payload of a WebSocket order book message.

    Attributes
    ----------
    symbol : str
        Trading pair.
    buyQuote : list[LmexWsOrderBookEntry]
        Bid levels (may be full snapshot or delta).
    sellQuote : list[LmexWsOrderBookEntry]
        Ask levels (may be full snapshot or delta).
    timestamp : int
        Unix timestamp in **milliseconds**.
    type : str
        ``"snapshot"`` for initial full book; ``"delta"`` for incremental.

    """

    symbol: str
    buyQuote: list[LmexWsOrderBookEntry]
    sellQuote: list[LmexWsOrderBookEntry]
    timestamp: int
    type: str = "snapshot"


class LmexWsOrderBookMsg(msgspec.Struct, frozen=True):
    """
    WebSocket message envelope for order book updates.

    Attributes
    ----------
    topic : str
        Topic string (e.g. ``"orderBookApi:BTC-USD_0"``).
    data : LmexWsOrderBookData
        Order book snapshot or delta payload.

    """

    topic: str
    data: LmexWsOrderBookData


# ---------------------------------------------------------------------------
# Order / execution events (private stream)
# ---------------------------------------------------------------------------
# Note: The private notification stream topic and auth mechanism are to be
# confirmed during implementation with sandbox credentials.


class LmexWsOrderEvent(msgspec.Struct):
    """
    A private order lifecycle event from the ``notificationsApi`` stream.

    Attributes
    ----------
    symbol : str
        Trading pair.
    orderId : int or str
        Exchange-assigned order identifier.
    clOrderId : str or None
        Client order identifier (if provided at submission).
    side : str
        ``"BUY"`` or ``"SELL"``.
    price : float or None
        Order price (``None`` for market orders).
    size : float
        Original order quantity.
    filledSize : float
        Cumulative filled quantity.
    status : int
        LMEX order status code (see ``LmexOrderStatus``).
    timestamp : int
        Event timestamp in **milliseconds**.
    avgFillPrice : float or None
        Average execution price (for fills).
    feeAmount : float or None
        Fee paid for the last fill.
    feeCurrency : str or None
        Currency the fee was paid in.
    tradeId : int or None
        Fill trade identifier (present only for fill events).

    """

    symbol: str
    orderId: int | str
    status: int
    side: str
    size: float
    filledSize: float
    timestamp: int
    clOrderId: str | None = None
    price: float | None = None
    avgFillPrice: float | None = None
    feeAmount: float | None = None
    feeCurrency: str | None = None
    tradeId: int | None = None
    type: str | None = None


class LmexWsOrderEventMsg(msgspec.Struct):
    """
    WebSocket message envelope for private order events.

    Attributes
    ----------
    topic : str
        ``"notificationsApi"``.
    data : list[LmexWsOrderEvent]
        Order lifecycle events.

    """

    topic: str
    data: list[LmexWsOrderEvent]


# ---------------------------------------------------------------------------
# Generic envelope (used for initial dispatch)
# ---------------------------------------------------------------------------


class LmexWsMsg(msgspec.Struct):
    """
    Minimal envelope to determine the message type before full decoding.

    Used by the dispatcher to route messages to the correct typed decoder.

    Attributes
    ----------
    topic : str or None
        Present on data messages (trades, orderbook, notifications).
    event : str or None
        Present on control messages (subscribe ack, pong).

    """

    topic: str | None = None
    event: str | None = None
