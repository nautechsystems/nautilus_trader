# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from enum import Enum
from typing import Optional

import msgspec


class BetfairSide(Enum):
    """
    BetfairSide.
    """

    BACK = "BACK"
    LAY = "LAY"


class ExecutionStatus(Enum):
    """
    ExecutionStatus.
    """

    PENDING = "PENDING"
    EXECUTABLE = "EXECUTABLE"
    EXECUTION_COMPLETE = "EXECUTION_COMPLETE"
    EXPIRED = "EXPIRED"


class PersistenceType(Enum):
    """
    PersistenceType.
    """

    LAPSE = "LAPSE"
    PERSIST = "PERSIST"
    MARKET_ON_CLOSE = "MARKET_ON_CLOSE"


class OrderType(Enum):
    """
    OrderType.
    """

    LIMIT = "LIMIT"
    LIMIT_ON_CLOSE = "LIMIT_ON_CLOSE"
    MARKET_ON_CLOSE = "MARKET_ON_CLOSE"
    # Deprecated
    MARKET_AT_THE_CLOSE = "MARKET_AT_THE_CLOSE"
    LIMIT_AT_THE_CLOSE = "LIMIT_AT_THE_CLOSE"


class BetOutcome(Enum):
    """
    BetOutcome.
    """

    WON = "WON"
    LOST = "LOST"


class ClearedOrder(msgspec.Struct):
    """
    ClearedOrder.
    """

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
    """
    ClearedOrdersResponse.
    """

    clearedOrders: list[ClearedOrder]
    moreAvailable: bool
