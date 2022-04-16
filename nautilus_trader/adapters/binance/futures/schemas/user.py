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
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesEventType
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesOrderType
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesPositionSide
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesPositionUpdateReason
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesTimeInForce
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesWorkingType


################################################################################
# WebSocket messages
################################################################################


class BinanceFuturesUserMsgData(msgspec.Struct):
    """
    Inner struct for execution WebSocket messages from `Binance`
    """

    e: BinanceFuturesEventType


class BinanceFuturesUserMsgWrapper(msgspec.Struct):
    """
    Provides a wrapper for execution WebSocket messages from `Binance`.
    """

    stream: str
    data: BinanceFuturesUserMsgData


class MarginCallPosition(msgspec.Struct):
    """Inner struct position for `Binance Futures` Margin Call events."""

    s: str  # Symbol
    ps: BinanceFuturesPositionSide  # Position Side
    pa: str  # Position  Amount
    mt: str  # Margin Type
    iw: str  # Isolated Wallet(if isolated position)
    mp: str  # MarkPrice
    up: str  # Unrealized PnL
    mm: str  # Maintenance Margin Required


class BinanceFuturesMarginCallMsg(msgspec.Struct):
    """WebSocket message for `Binance Futures` Margin Call events."""

    e: str  # Event Type
    E: int  # Event Time
    cw: float  # Cross Wallet Balance. Only pushed with crossed position margin call
    p: List[MarginCallPosition]


class BinanceFuturesBalance(msgspec.Struct):
    """Inner struct balance for `Binance Futures` Balance and Position update event."""

    a: str  # Asset
    wb: str  # Wallet Balance
    cw: str  # Cross Wallet Balance
    bc: str  # Balance Change except PnL and Commission


class BinanceFuturesPosition(msgspec.Struct):
    """Inner struct position for `Binance Futures` Balance and Position update event."""

    s: str  # Symbol
    pa: str  # Position amount
    ep: str  # Entry price
    cr: str  # (Pre-free) Accumulated Realized
    up: str  # Unrealized PnL
    mt: str  # Margin type
    iw: str  # Isolated wallet
    ps: BinanceFuturesPositionSide


class BinanceFuturesAccountUpdateData(msgspec.Struct):
    """WebSocket message for `Binance Futures` Balance and Position Update events."""

    m: BinanceFuturesPositionUpdateReason
    B: List[BinanceFuturesBalance]
    P: List[BinanceFuturesPosition]


class BinanceFuturesAccountUpdateMsg(msgspec.Struct):
    """WebSocket message for `Binance Futures` Balance and Position Update events."""

    e: str  # Event Type
    E: int  # Event Time
    T: int  # Transaction Time
    a: BinanceFuturesAccountUpdateData


class BinanceFuturesAccountUpdateWrapper(msgspec.Struct):
    """WebSocket message wrapper for `Binance Futures` Balance and Position Update events."""

    stream: str
    data: BinanceFuturesAccountUpdateMsg


class BinanceFuturesOrderData(msgspec.Struct):
    """
    WebSocket message 'inner struct' for `Binance Futures` Order Update events.

    Client Order ID 'c':
     - starts with "autoclose-": liquidation order/
     - starts with "adl_autoclose": ADL auto close order/
    """

    s: str  # Symbol
    c: str  # Client Order ID
    S: BinanceOrderSide
    o: BinanceFuturesOrderType
    f: BinanceFuturesTimeInForce
    q: str  # Original Quantity
    p: str  # Original Price
    ap: str  # Average Price
    sp: Optional[str] = None  # Stop Price. Please ignore with TRAILING_STOP_MARKET order
    x: BinanceExecutionType
    X: BinanceOrderStatus
    i: int  # Order ID
    l: str  # Order Last Filled Quantity
    z: str  # Order Filled Accumulated Quantity
    L: str  # Last Filled Price
    N: Optional[str] = None  # Commission Asset, will not push if no commission
    n: Optional[str] = None  # Commission, will not push if no commission
    T: int  # Order Trade Time
    t: int  # Trade ID
    b: str  # Bids Notional
    a: str  # Ask Notional
    m: bool  # Is trade the maker side
    R: bool  # Is reduce only
    wt: BinanceFuturesWorkingType
    ot: BinanceFuturesOrderType
    ps: BinanceFuturesPositionSide
    cp: Optional[bool] = None  # If Close-All, pushed with conditional order
    AP: Optional[str] = None  # Activation Price, only pushed with TRAILING_STOP_MARKET order
    cr: Optional[str] = None  # Callback Rate, only pushed with TRAILING_STOP_MARKET order
    pP: bool  # ignore
    si: int  # ignore
    ss: int  # ignore
    rp: str  # Realized Profit of the trade


class BinanceFuturesOrderUpdateMsg(msgspec.Struct):
    """WebSocket message for `Binance Futures` Order Update events."""

    e: str  # Event Type
    E: int  # Event Time
    T: int  # Transaction Time
    o: BinanceFuturesOrderData


class BinanceFuturesOrderUpdateWrapper(msgspec.Struct):
    """WebSocket message wrapper for `Binance Futures` Order Update events."""

    stream: str
    data: BinanceFuturesOrderUpdateMsg
