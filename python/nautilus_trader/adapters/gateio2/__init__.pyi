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
Type stubs for Gate.io Rust bindings.
"""

from typing import Any

from nautilus_trader.model.instruments import Instrument

class GateioHttpClient:
    """
    Rust HTTP client for Gate.io API.
    """

    def __init__(
        self,
        base_http_url: str | None = None,
        base_ws_spot_url: str | None = None,
        base_ws_futures_url: str | None = None,
        base_ws_options_url: str | None = None,
        api_key: str | None = None,
        api_secret: str | None = None,
    ) -> None: ...
    async def load_instruments(self) -> list[Instrument]:
        """Load all instruments from Gate.io."""
        ...
    async def instruments(self) -> list[Instrument]:
        """Get loaded instruments."""
        ...

class GateioWebSocketClient:
    """
    Rust WebSocket client for Gate.io.
    """

    def __init__(
        self,
        base_http_url: str | None = None,
        base_ws_spot_url: str | None = None,
        base_ws_futures_url: str | None = None,
        base_ws_options_url: str | None = None,
        api_key: str | None = None,
        api_secret: str | None = None,
    ) -> None: ...
    async def subscribe_spot_ticker(self, currency_pair: str) -> None:
        """Subscribe to spot ticker channel."""
        ...
    async def subscribe_spot_order_book(self, currency_pair: str) -> None:
        """Subscribe to spot order book channel."""
        ...
    async def subscribe_spot_trades(self, currency_pair: str) -> None:
        """Subscribe to spot trades channel."""
        ...
    async def subscribe_futures_ticker(self, contract: str) -> None:
        """Subscribe to futures ticker channel."""
        ...
    async def subscribe_futures_order_book(self, contract: str) -> None:
        """Subscribe to futures order book channel."""
        ...
    async def subscription_count(self) -> int:
        """Get number of active subscriptions."""
        ...
    async def subscriptions(self) -> list[str]:
        """Get all active subscriptions."""
        ...

class GateioMarketType:
    """Gate.io market type enum."""

    SPOT: GateioMarketType
    MARGIN: GateioMarketType
    FUTURES: GateioMarketType
    DELIVERY: GateioMarketType
    OPTIONS: GateioMarketType

class GateioOrderSide:
    """Gate.io order side enum."""

    BUY: GateioOrderSide
    SELL: GateioOrderSide

class GateioOrderType:
    """Gate.io order type enum."""

    LIMIT: GateioOrderType
    MARKET: GateioOrderType

class GateioTimeInForce:
    """Gate.io time in force enum."""

    GTC: GateioTimeInForce
    IOC: GateioTimeInForce
    POC: GateioTimeInForce
    FOK: GateioTimeInForce

class GateioOrderStatus:
    """Gate.io order status enum."""

    OPEN: GateioOrderStatus
    CLOSED: GateioOrderStatus
    CANCELLED: GateioOrderStatus

class GateioAccountType:
    """Gate.io account type enum."""

    SPOT: GateioAccountType
    MARGIN: GateioAccountType
    FUTURES: GateioAccountType
    DELIVERY: GateioAccountType
    OPTIONS: GateioAccountType
    UNIFIED: GateioAccountType
