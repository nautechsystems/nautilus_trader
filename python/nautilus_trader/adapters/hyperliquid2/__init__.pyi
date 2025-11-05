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
"""
Type stubs for Hyperliquid2 Rust bindings.
"""

from typing import Any

class Hyperliquid2HttpClient:
    """
    Rust HTTP client for Hyperliquid API.
    """

    def __init__(
        self,
        private_key: str | None = None,
        http_base: str | None = None,
        testnet: bool = False,
    ) -> None: ...
    async def load_instruments(self) -> int:
        """
        Load all instruments from Hyperliquid.

        Returns the count of loaded instruments.
        """
        ...
    async def request_meta_info(self) -> dict[str, Any]:
        """Fetch meta information (universe of assets)."""
        ...
    async def request_all_mids(self) -> dict[str, Any]:
        """Fetch all mid prices."""
        ...
    async def request_l2_book(self, coin: str) -> dict[str, Any]:
        """Fetch L2 order book for a specific coin."""
        ...
    async def request_trades(self, coin: str) -> list[dict[str, Any]]:
        """Fetch recent trades for a specific coin."""
        ...
    async def request_user_state(self, user: str) -> dict[str, Any]:
        """Fetch user state (positions, balances)."""
        ...
    async def request_open_orders(self, user: str) -> list[dict[str, Any]]:
        """Fetch open orders for a user."""
        ...
    async def request_user_fills(self, user: str) -> list[dict[str, Any]]:
        """Fetch user fills (trade history)."""
        ...

class Hyperliquid2WebSocketClient:
    """
    Rust WebSocket client for Hyperliquid.
    """

    def __init__(
        self,
        ws_base: str | None = None,
        testnet: bool = False,
    ) -> None: ...
    async def connect(self) -> None:
        """Connect to the WebSocket."""
        ...
    async def subscribe_all_mids(self) -> None:
        """Subscribe to all mids channel."""
        ...
    async def subscribe_trades(self, coin: str) -> None:
        """Subscribe to trades channel for a specific coin."""
        ...
    async def subscribe_l2_book(self, coin: str) -> None:
        """Subscribe to L2 book channel for a specific coin."""
        ...
    async def subscribe_candle(self, coin: str, interval: str) -> None:
        """Subscribe to candle channel for a specific coin and interval."""
        ...
    async def subscribe_user(self, user: str) -> None:
        """Subscribe to user events channel."""
        ...
    async def subscribe_user_fills(self, user: str) -> None:
        """Subscribe to user fills channel."""
        ...
    async def receive(self) -> str | None:
        """Receive a message from the WebSocket."""
        ...
    async def unsubscribe_trades(self, coin: str) -> None:
        """Unsubscribe from trades channel."""
        ...
    async def unsubscribe_l2_book(self, coin: str) -> None:
        """Unsubscribe from L2 book channel."""
        ...

class Hyperliquid2OrderSide:
    """Hyperliquid2 order side enum."""

    BUY: Hyperliquid2OrderSide
    SELL: Hyperliquid2OrderSide

class Hyperliquid2OrderType:
    """Hyperliquid2 order type enum."""

    LIMIT: Hyperliquid2OrderType
    MARKET: Hyperliquid2OrderType
    STOP_MARKET: Hyperliquid2OrderType
    STOP_LIMIT: Hyperliquid2OrderType
    TAKE_PROFIT_MARKET: Hyperliquid2OrderType
    TAKE_PROFIT_LIMIT: Hyperliquid2OrderType

class Hyperliquid2TimeInForce:
    """Hyperliquid2 time in force enum."""

    GTC: Hyperliquid2TimeInForce
    IOC: Hyperliquid2TimeInForce
    ALO: Hyperliquid2TimeInForce

class Hyperliquid2OrderStatus:
    """Hyperliquid2 order status enum."""

    OPEN: Hyperliquid2OrderStatus
    FILLED: Hyperliquid2OrderStatus
    CANCELED: Hyperliquid2OrderStatus
    REJECTED: Hyperliquid2OrderStatus
    TRIGGERED: Hyperliquid2OrderStatus
