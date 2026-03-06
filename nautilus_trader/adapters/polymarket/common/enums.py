from __future__ import annotations

from enum import Enum
from enum import unique


@unique
class PolymarketSignatureType(Enum):
    EOA = 0  # EIP712 signature signed by an EOA
    POLY_PROXY = 1  # EIP712 signature (Polymarket proxy wallet)
    POLY_GNOSIS_SAFE = 2  # EIP712 signature (Polymarket gnosis safe wallet)


@unique
class PolymarketOrderSide(Enum):
    BUY = "BUY"
    SELL = "SELL"


@unique
class PolymarketLiquiditySide(Enum):
    MAKER = "MAKER"
    TAKER = "TAKER"


@unique
class PolymarketOrderType(Enum):
    FOK = "FOK"
    FAK = "FAK"  # IOC
    GTC = "GTC"
    GTD = "GTD"


@unique
class PolymarketEventType(Enum):
    PLACEMENT = "PLACEMENT"
    UPDATE = "UPDATE"  # Emitted for a MATCH
    CANCELLATION = "CANCELLATION"
    TRADE = "TRADE"


@unique
class PolymarketOrderStatus(Enum):
    # Order was invalid
    INVALID = "INVALID"

    # Order placed and live
    LIVE = "LIVE"

    # Order marketable, but subject to matching delay
    DELAYED = "DELAYED"

    # Order matched (marketable)
    MATCHED = "MATCHED"

    # Order marketable, but failure delaying, placement not successful
    UNMATCHED = "UNMATCHED"

    # Order canceled
    CANCELED = "CANCELED"

    # Order canceled as market resolved
    CANCELED_MARKET_RESOLVED = "CANCELED_MARKET_RESOLVED"


@unique
class PolymarketTradeStatus(Enum):
    # Trade has been matched and sent to the executor service by the operator,
    # the executor service submits the trade as a transaction to the Exchange contract.
    MATCHED = "MATCHED"

    # Trade is observed to be mined into the chain, no finality threshold established
    MINED = "MINED"

    # Trade has achieved strong probabilistic finality and was successful
    CONFIRMED = "CONFIRMED"

    # Trade transaction has failed (revert or reorg) and is being retried/resubmitted by the operator
    RETRYING = "RETRYING"

    # Trade has failed and is not being retried
    FAILED = "FAILED"
