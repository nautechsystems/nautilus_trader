# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
