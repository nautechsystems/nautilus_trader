from enum import Enum
from typing import Optional

import msgspec


class BetfairSide(Enum):
    """BetfairSide"""

    BACK = "BACK"
    LAY = "LAY"


class ExecutionStatus(Enum):
    """ExecutionStatus"""

    PENDING = "PENDING"
    EXECUTABLE = "EXECUTABLE"
    EXECUTION_COMPLETE = "EXECUTION_COMPLETE"
    EXPIRED = "EXPIRED"


class PersistenceType(Enum):
    """PersistenceType"""

    LAPSE = "LAPSE"
    PERSIST = "PERSIST"
    MARKET_ON_CLOSE = "MARKET_ON_CLOSE"


class OrderType(Enum):
    """OrderType"""

    LIMIT = "LIMIT"
    LIMIT_ON_CLOSE = "LIMIT_ON_CLOSE"
    MARKET_ON_CLOSE = "MARKET_ON_CLOSE"
    # Deprecated
    MARKET_AT_THE_CLOSE = "MARKET_AT_THE_CLOSE"
    LIMIT_AT_THE_CLOSE = "LIMIT_AT_THE_CLOSE"


class BetOutcome(Enum):
    """BetOutcome"""

    WON = "WON"
    LOST = "LOST"


class ClearedOrder(msgspec.Struct):
    """ClearedOrder"""

    eventTypeId: str
    eventId: str
    marketId: str
    selectionId: int
    handicap: float
    betId: str
    placedDate: str
    persistenceType: PersistenceType
    orderType: OrderType
    side: BetfairSide
    betOutcome: BetOutcome
    priceRequested: float
    settledDate: str
    lastMatchedDate: str
    betCount: int
    priceMatched: float
    priceReduced: bool
    sizeSettled: float
    profit: float
    customerOrderRef: Optional[str] = None
    customerStrategyRef: Optional[str] = None


class ClearedOrdersResponse(msgspec.Struct):
    """ClearedOrdersResponse"""

    clearedOrders: list[ClearedOrder]
    moreAvailable: bool
