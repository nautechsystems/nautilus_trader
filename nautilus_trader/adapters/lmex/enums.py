# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter
#  Licensed under the GNU Lesser General Public License Version 3.0
# -------------------------------------------------------------------------------------------------

from enum import IntEnum


class LmexOrderStatus(IntEnum):
    """
    Represents LMEX order status codes returned in REST responses and WebSocket events.

    References
    ----------
    https://lmex.io/apidocs/spot/v3.2/
    """

    ORDER_INSERTED = 2
    """Order accepted by the exchange and resting in the book."""

    ORDER_FULLY_TRANSACTED = 4
    """Order completely filled."""

    ORDER_PARTIALLY_TRANSACTED = 5
    """Order partially filled; still active."""

    ORDER_CANCELLED = 6
    """Order cancelled by user or system."""

    STATUS_INACTIVE = 7
    """Order is inactive (expired or system-cancelled)."""

    TRIGGER_INSERTED = 8
    """Trigger (stop/conditional) order accepted."""

    TRIGGER_ACTIVATED = 9
    """Trigger condition met; underlying order submitted."""

    MARKET_UNAVAILABLE = 10
    """Order rejected because the market is not available."""

    REJECT_AMEND_ORDER_REJECTION = 15
    """Amend (modify) request was rejected."""

    FAILED_ERROR = 16
    """Order submission failed due to a system error."""


class LmexOrderSide(str):
    """
    LMEX order side string constants.

    LMEX uses "BUY" and "SELL" strings in REST and WebSocket payloads.
    """

    BUY: str = "BUY"
    SELL: str = "SELL"


class LmexOrderType(str):
    """
    LMEX order type string constants.
    """

    LIMIT: str = "LIMIT"
    MARKET: str = "MARKET"
    STOP: str = "STOP"
    STOP_LIMIT: str = "STOP_LIMIT"
