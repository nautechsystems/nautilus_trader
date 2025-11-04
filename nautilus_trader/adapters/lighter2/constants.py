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

from typing import Final

from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue


LIGHTER: Final[str] = "LIGHTER"
LIGHTER_VENUE: Final[Venue] = Venue(LIGHTER)
LIGHTER_CLIENT_ID: Final[ClientId] = ClientId(LIGHTER)

# Lighter API endpoints
LIGHTER_MAINNET_HTTP_URL: Final[str] = "https://api.lighter.xyz"
LIGHTER_TESTNET_HTTP_URL: Final[str] = "https://api-testnet.lighter.xyz"
LIGHTER_MAINNET_WS_URL: Final[str] = "wss://api.lighter.xyz/ws"
LIGHTER_TESTNET_WS_URL: Final[str] = "wss://api-testnet.lighter.xyz/ws"

# Order types
LIGHTER_ORDER_TYPE_LIMIT: Final[str] = "limit"
LIGHTER_ORDER_TYPE_MARKET: Final[str] = "market"
LIGHTER_ORDER_TYPE_STOP_LOSS: Final[str] = "stop_loss"
LIGHTER_ORDER_TYPE_TAKE_PROFIT: Final[str] = "take_profit"
LIGHTER_ORDER_TYPE_TWAP: Final[str] = "twap"

# Order sides
LIGHTER_ORDER_SIDE_BUY: Final[str] = "buy"
LIGHTER_ORDER_SIDE_SELL: Final[str] = "sell"

# Order statuses
LIGHTER_ORDER_STATUS_PENDING: Final[str] = "pending"
LIGHTER_ORDER_STATUS_FILLED: Final[str] = "filled"
LIGHTER_ORDER_STATUS_PARTIALLY_FILLED: Final[str] = "partially_filled"
LIGHTER_ORDER_STATUS_CANCELLED: Final[str] = "cancelled"
LIGHTER_ORDER_STATUS_REJECTED: Final[str] = "rejected"

# WebSocket channels
LIGHTER_WS_CHANNEL_ORDERBOOK: Final[str] = "orderbook"
LIGHTER_WS_CHANNEL_TRADES: Final[str] = "trades"
LIGHTER_WS_CHANNEL_ACCOUNT: Final[str] = "account"
LIGHTER_WS_CHANNEL_ORDERS: Final[str] = "orders"

# API rate limits (requests per second)
LIGHTER_RATE_LIMIT_MARKET_DATA: Final[int] = 10
LIGHTER_RATE_LIMIT_TRADING: Final[int] = 5
LIGHTER_RATE_LIMIT_ACCOUNT: Final[int] = 5

# Precision constants
LIGHTER_DEFAULT_PRICE_PRECISION: Final[int] = 8
LIGHTER_DEFAULT_SIZE_PRECISION: Final[int] = 8