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

import asyncio
import json
import os
from typing import Any, Callable

import msgspec

from nautilus_trader.adapters.lighter2.constants import LIGHTER_MAINNET_WS_URL
from nautilus_trader.adapters.lighter2.constants import LIGHTER_TESTNET_WS_URL
from nautilus_trader.adapters.lighter2.constants import LIGHTER_WS_CHANNEL_ACCOUNT
from nautilus_trader.adapters.lighter2.constants import LIGHTER_WS_CHANNEL_ORDERBOOK
from nautilus_trader.adapters.lighter2.constants import LIGHTER_WS_CHANNEL_ORDERS
from nautilus_trader.adapters.lighter2.constants import LIGHTER_WS_CHANNEL_TRADES
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.core.nautilus_pyo3 import WebSocketClient
from nautilus_trader.core.nautilus_pyo3 import WebSocketConfig


class LighterWebSocketClient:
    """
    Provides a WebSocket client for the Lighter exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.
    handler : Callable[[bytes], None]
        The callback handler for message events.
    api_key_private_key : str, optional
        The Lighter API private key.
    eth_private_key : str, optional
        The Ethereum private key for signing transactions.
    base_url : str, optional
        The base URL for the WebSocket client.
    is_testnet : bool, default False
        If the client should connect to testnet.
    proxy_url : str, optional
        The proxy URL for the WebSocket client.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        clock: LiveClock,
        logger: Logger,
        handler: Callable[[bytes], None],
        api_key_private_key: str | None = None,
        eth_private_key: str | None = None,
        base_url: str | None = None,
        is_testnet: bool = False,
        proxy_url: str | None = None,
    ) -> None:
        self._loop = loop
        self._clock = clock
        self._log = logger
        self._handler = handler

        # API credentials
        self._api_key_private_key = api_key_private_key or os.environ.get("LIGHTER_API_KEY_PRIVATE_KEY")
        self._eth_private_key = eth_private_key or os.environ.get("LIGHTER_ETH_PRIVATE_KEY")

        # Base URL configuration
        if base_url is None:
            base_url = LIGHTER_TESTNET_WS_URL if is_testnet else LIGHTER_MAINNET_WS_URL
        self._base_url = base_url
        self._is_testnet = is_testnet

        # WebSocket client
        self._client: WebSocketClient | None = None
        self._is_connected = False

        # Subscription management
        self._subscriptions: set[str] = set()
        self._subscription_id = 1

        # Message handling
        self._message_handlers: dict[str, Callable[[dict], None]] = {
            LIGHTER_WS_CHANNEL_ORDERBOOK: self._handle_orderbook_message,
            LIGHTER_WS_CHANNEL_TRADES: self._handle_trades_message,
            LIGHTER_WS_CHANNEL_ACCOUNT: self._handle_account_message,
            LIGHTER_WS_CHANNEL_ORDERS: self._handle_orders_message,
        }

    async def connect(self) -> None:
        """Connect to the WebSocket server."""
        if self._is_connected:
            self._log.warning("WebSocket already connected")
            return

        try:
            # Prepare headers for authentication if needed
            headers = []
            if self._api_key_private_key:
                # Add authentication headers based on Lighter's requirements
                # This will need to be implemented based on Lighter's auth spec
                pass

            config = WebSocketConfig(
                url=self._base_url,
                handler=self._handle_message,
                heartbeat=30,
                headers=headers,
            )

            self._client = await WebSocketClient.connect(config=config)
            self._is_connected = True
            self._log.info(f"Connected to Lighter WebSocket: {self._base_url}")

        except Exception as e:
            self._log.error(f"Failed to connect to Lighter WebSocket: {e}")
            raise

    async def disconnect(self) -> None:
        """Disconnect from the WebSocket server."""
        if not self._is_connected or not self._client:
            self._log.warning("WebSocket not connected")
            return

        try:
            await self._client.disconnect()
            self._log.info("Disconnected from Lighter WebSocket")
        except Exception as e:
            self._log.error(f"Error disconnecting from Lighter WebSocket: {e}")
        finally:
            self._client = None
            self._is_connected = False
            self._subscriptions.clear()

    async def subscribe_orderbook(self, instrument_id: str, depth: int = 20) -> None:
        """
        Subscribe to order book updates.

        Parameters
        ----------
        instrument_id : str
            The instrument identifier.
        depth : int, default 20
            The order book depth.

        """
        subscription = {
            "id": self._subscription_id,
            "method": "subscribe",
            "params": {
                "channel": LIGHTER_WS_CHANNEL_ORDERBOOK,
                "instrument_id": instrument_id,
                "depth": depth,
            }
        }

        await self._send_message(subscription)
        self._subscriptions.add(f"{LIGHTER_WS_CHANNEL_ORDERBOOK}:{instrument_id}")
        self._subscription_id += 1

    async def subscribe_trades(self, instrument_id: str) -> None:
        """
        Subscribe to trade updates.

        Parameters
        ----------
        instrument_id : str
            The instrument identifier.

        """
        subscription = {
            "id": self._subscription_id,
            "method": "subscribe",
            "params": {
                "channel": LIGHTER_WS_CHANNEL_TRADES,
                "instrument_id": instrument_id,
            }
        }

        await self._send_message(subscription)
        self._subscriptions.add(f"{LIGHTER_WS_CHANNEL_TRADES}:{instrument_id}")
        self._subscription_id += 1

    async def subscribe_account_updates(self) -> None:
        """Subscribe to account updates."""
        subscription = {
            "id": self._subscription_id,
            "method": "subscribe",
            "params": {
                "channel": LIGHTER_WS_CHANNEL_ACCOUNT,
            }
        }

        await self._send_message(subscription)
        self._subscriptions.add(LIGHTER_WS_CHANNEL_ACCOUNT)
        self._subscription_id += 1

    async def subscribe_order_updates(self) -> None:
        """Subscribe to order updates."""
        subscription = {
            "id": self._subscription_id,
            "method": "subscribe",
            "params": {
                "channel": LIGHTER_WS_CHANNEL_ORDERS,
            }
        }

        await self._send_message(subscription)
        self._subscriptions.add(LIGHTER_WS_CHANNEL_ORDERS)
        self._subscription_id += 1

    async def unsubscribe(self, channel: str, instrument_id: str | None = None) -> None:
        """
        Unsubscribe from a channel.

        Parameters
        ----------
        channel : str
            The channel to unsubscribe from.
        instrument_id : str, optional
            The instrument identifier (if applicable).

        """
        unsubscription = {
            "id": self._subscription_id,
            "method": "unsubscribe",
            "params": {
                "channel": channel,
            }
        }

        if instrument_id:
            unsubscription["params"]["instrument_id"] = instrument_id

        await self._send_message(unsubscription)
        
        # Remove from subscriptions
        subscription_key = f"{channel}:{instrument_id}" if instrument_id else channel
        self._subscriptions.discard(subscription_key)
        self._subscription_id += 1

    async def _send_message(self, message: dict[str, Any]) -> None:
        """Send a message to the WebSocket server."""
        if not self._client:
            raise RuntimeError("WebSocket not connected")

        try:
            message_bytes = msgspec.json.encode(message)
            await self._client.send_text(message_bytes)
            self._log.debug(f"Sent WebSocket message: {message}")
        except Exception as e:
            self._log.error(f"Error sending WebSocket message: {e}")
            raise

    def _handle_message(self, raw: bytes) -> None:
        """Handle incoming WebSocket messages."""
        self._loop.create_task(self._process_message(raw))

    async def _process_message(self, raw: bytes) -> None:
        """Process incoming WebSocket messages."""
        try:
            message_str = raw.decode('utf-8')
            message = json.loads(message_str)
            
            # Handle subscription confirmations
            if "id" in message and "result" in message:
                self._handle_subscription_response(message)
                return

            # Handle channel data
            channel = message.get("channel")
            if channel in self._message_handlers:
                self._message_handlers[channel](message)
            else:
                self._log.warning(f"Unhandled channel: {channel}")

            # Forward to main handler
            self._handler(raw)

        except Exception as e:
            self._log.error(f"Error processing WebSocket message: {e}")

    def _handle_subscription_response(self, message: dict[str, Any]) -> None:
        """Handle subscription response messages."""
        if message.get("result") == "success":
            self._log.info(f"WebSocket subscription successful: ID {message.get('id')}")
        else:
            self._log.error(f"WebSocket subscription failed: {message}")

    def _handle_orderbook_message(self, message: dict[str, Any]) -> None:
        """Handle order book messages."""
        instrument_id = message.get("instrument_id")
        data = message.get("data", {})
        
        self._log.debug(f"Received orderbook update for {instrument_id}: "
                       f"bids={len(data.get('bids', []))}, "
                       f"asks={len(data.get('asks', []))}")

    def _handle_trades_message(self, message: dict[str, Any]) -> None:
        """Handle trade messages."""
        instrument_id = message.get("instrument_id")
        trades = message.get("data", [])
        
        self._log.debug(f"Received {len(trades)} trades for {instrument_id}")

    def _handle_account_message(self, message: dict[str, Any]) -> None:
        """Handle account messages."""
        data = message.get("data", {})
        self._log.debug(f"Received account update: {data}")

    def _handle_orders_message(self, message: dict[str, Any]) -> None:
        """Handle order messages."""
        data = message.get("data", {})
        order_id = data.get("order_id")
        status = data.get("status")
        
        self._log.debug(f"Received order update: {order_id} -> {status}")

    @property
    def is_connected(self) -> bool:
        """Check if the WebSocket is connected."""
        return self._is_connected

    @property
    def subscriptions(self) -> set[str]:
        """Get current subscriptions."""
        return self._subscriptions.copy()