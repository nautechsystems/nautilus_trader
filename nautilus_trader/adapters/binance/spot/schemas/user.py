# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import List, Optional

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceExecutionType
from nautilus_trader.adapters.binance.common.enums import BinanceOrderSide
from nautilus_trader.adapters.binance.common.enums import BinanceOrderStatus
from nautilus_trader.adapters.binance.spot.enums import BinanceSpotEventType
from nautilus_trader.adapters.binance.spot.enums import BinanceSpotOrderType
from nautilus_trader.adapters.binance.spot.enums import BinanceSpotTimeInForce


################################################################################
# WebSocket messages
################################################################################


class BinanceSpotUserMsgData(msgspec.Struct):
    """
    Inner struct for execution WebSocket messages from `Binance`
    """

    e: BinanceSpotEventType


class BinanceSpotUserMsgWrapper(msgspec.Struct):
    """
    Provides a wrapper for execution WebSocket messages from `Binance`.
    """

    stream: str
    data: BinanceSpotUserMsgData


class BinanceSpotBalance(msgspec.Struct):
    """Inner struct for `Binance Spot/Margin` balances."""

    a: str  # Asset
    f: str  # Free
    l: str  # Locked


class BinanceSpotAccountUpdateMsg(msgspec.Struct):
    """WebSocket message for `Binance Spot/Margin` Account Update events."""

    e: str  # Event Type
    E: int  # Event Time
    u: int  # Transaction Time
    B: List[BinanceSpotBalance]


class BinanceSpotAccountUpdateWrapper(msgspec.Struct):
    """WebSocket message wrapper for `Binance Spot/Margin` Account Update events."""

    stream: str
    data: BinanceSpotAccountUpdateMsg


class BinanceSpotOrderUpdateData(msgspec.Struct):
    """
    WebSocket message 'inner struct' for `Binance Spot/Margin` Order Update events.

    """

    e: BinanceSpotEventType
    E: int  # Event time
    s: str  # Symbol
    c: str  # Client order ID
    S: BinanceOrderSide
    o: BinanceSpotOrderType
    f: BinanceSpotTimeInForce
    q: str  # Original Quantity
    p: str  # Original Price
    P: str  # Stop price
    F: str  # Iceberg quantity
    g: int  # Order list ID
    C: str  # Original client order ID; This is the ID of the order being canceled
    x: BinanceExecutionType
    X: BinanceOrderStatus
    r: str  # Order reject reason; will be an error code
    i: int  # Order ID
    l: str  # Order Last Filled Quantity
    z: str  # Order Filled Accumulated Quantity
    L: str  # Last Filled Price
    n: Optional[str] = None  # Commission, will not push if no commission
    N: Optional[str] = None  # Commission Asset, will not push if no commission
    T: int  # Order Trade Time
    t: int  # Trade ID
    I: int  # Ignore
    w: bool  # Is the order on the book?
    m: bool  # Is trade the maker side
    M: bool  # Ignore
    O: int  # Order creation time
    Z: str  # Cumulative quote asset transacted quantity
    Y: str  # Last quote asset transacted quantity (i.e. lastPrice * lastQty)
    Q: str  # Quote Order Qty


class BinanceSpotOrderUpdateWrapper(msgspec.Struct):
    """WebSocket message wrapper for `Binance Spot/Margin` Order Update events."""

    stream: str
    data: BinanceSpotOrderUpdateData
