# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter
#  Licensed under the GNU Lesser General Public License Version 3.0
# -------------------------------------------------------------------------------------------------

"""
msgspec Struct definitions for LMEX order management REST responses.

All field names match the **exact** JSON keys returned by the live API
(verified against ``https://test-api.lmex.io/spot`` on 2026-05-26).

Key quirks:
- ``POST /api/v3.2/order`` and ``DELETE /api/v3.2/order`` return a **list**,
  not a single object.
- The order identifier field is ``orderID`` (capital D), not ``orderId``.
- ``clOrderID`` (capital D) is used in submit/cancel responses and open orders.
  ``clOrderID`` (capital D) is also used in fills, but the fill's order reference
  field is ``orderId`` (lowercase d) — an inconsistency in the LMEX API.
- ``orderType`` is an integer (76 = LIMIT, 77 = MARKET) in REST responses.
"""

from __future__ import annotations

import msgspec


class LmexOrderResponse(msgspec.Struct, gc=False):
    """
    One entry returned by ``POST /api/v3.2/order`` and ``DELETE /api/v3.2/order``.

    The REST response is a **list**; callers should decode as
    ``list[LmexOrderResponse]`` and take the first element.

    Attributes
    ----------
    symbol : str
        Trading pair (e.g. ``"BTC-USD"``).
    orderID : str
        Exchange-assigned UUID order identifier.
    clOrderID : str or None
        Client-assigned order ID echoed back, or ``None`` if not supplied.
    orderType : int
        Numeric order type (76 = LIMIT, 77 = MARKET).
    price : float
        Order price (0 for market orders).
    size : float
        Original order quantity.
    fillSize : float
        Quantity filled so far (alias for ``filledSize`` in this endpoint).
    status : int
        LMEX order status code (see ``LmexOrderStatus``).
    side : str
        ``"BUY"`` or ``"SELL"``.
    timestamp : int
        Order creation timestamp in **milliseconds**.
    averageFillPrice : float
        Volume-weighted average fill price (0 if no fills yet).

    """

    symbol: str
    orderID: str
    status: int
    side: str
    size: float
    timestamp: int
    # Frequently absent on some response shapes → default to safe values
    orderType: int = 0
    price: float = 0.0
    fillSize: float = 0.0
    clOrderID: str | None = None
    averageFillPrice: float = 0.0
    originalSize: float = 0.0
    remainingSize: float = 0.0
    triggerPrice: float = 0.0
    stopPrice: float | None = None
    trigger: bool = False
    message: str = ""
    stealth: int | None = None
    deviation: int | None = None
    postOnly: bool = False
    orderDetailType: str | None = None
    userCurrency: str | None = None
    time_in_force: str | None = None


class LmexOpenOrder(msgspec.Struct, gc=False):
    """
    One entry from ``GET /api/v3.2/user/open_orders``.

    Note
    ----
    This endpoint returns ``orderState`` (string) rather than an integer
    ``status`` field.  The ``orderID`` field is a UUID string.

    """

    symbol: str
    orderID: str
    side: str
    size: float
    orderType: int = 0
    price: float = 0.0
    filledSize: float = 0.0
    fillSize: float = 0.0
    orderValue: float = 0.0
    remainingSize: float = 0.0
    averageFillPrice: float = 0.0
    timestamp: int = 0
    clOrderID: str | None = None
    orderState: str = ""
    timeInForce: str = "GTC"
    postOnly: bool = False
    triggerOrder: bool = False
    triggerPrice: float = 0.0
    quote: str = ""
    requestId: int | None = None
    orderDetailType: str | None = None


class LmexFill(msgspec.Struct, gc=False):
    """
    A single trade fill from ``GET /api/v3.2/user/trade_history``.

    Note
    ----
    ``tradeId`` and ``orderId`` are UUID strings.  ``orderId`` uses a lowercase
    ``d`` — this is an inconsistency in the LMEX API (submit/cancel responses
    use ``orderID`` with a capital ``D``).

    """

    symbol: str
    orderId: str          # lowercase 'd' — inconsistent with other endpoints
    tradeId: str          # UUID string
    side: str
    price: float          # original order price
    size: float           # original order size
    filledSize: float
    filledPrice: float
    feeCurrency: str
    feeAmount: float
    timestamp: int
    serialId: int = 0
    base: str = ""
    quote: str = ""
    clOrderID: str | None = None
    averageFillPrice: float = 0.0
    realizedPnl: float = 0.0
    total: float = 0.0
    wallet: str = ""
    orderType: int = 0
    triggerType: int = 0
    triggerPrice: float = 0.0


class LmexWalletEntry(msgspec.Struct, gc=False):
    """
    One currency entry from ``GET /api/v3.2/user/wallet``.
    """

    currency: str
    available: float
    total: float
