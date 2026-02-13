# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
from collections.abc import Awaitable
from collections.abc import Callable
from enum import Enum
from enum import unique
from typing import Any
from weakref import WeakSet

import msgspec

from nautilus_trader.adapters.polymarket.common.credentials import PolymarketWebSocketAuth
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.nautilus_pyo3 import WebSocketClient
from nautilus_trader.core.nautilus_pyo3 import WebSocketClientError
from nautilus_trader.core.nautilus_pyo3 import WebSocketConfig
from nautilus_trader.live.cancellation import cancel_tasks_with_timeout


@unique
class PolymarketWebSocketChannel(Enum):
    MARKET = "market"
    USER = "user"


class PolymarketWebSocketClient:
    """
    Provides a Polymarket streaming WebSocket client.

    Manages a pool of WebSocket connections, each limited to a configurable number
    of subscriptions (default 200, Polymarket limit is 500).

    Parameters
    ----------
    clock : LiveClock
        The clock for the client.
    base_url : str, optional
        The base URL for the WebSocket connection.
    channel : PolymarketWebSocketChannel
        The channel type (MARKET or USER).
    handler : Callable[[bytes], None]
        The callback handler for message events.
    handler_reconnect : Callable[..., Awaitable[None]], optional
        The callback handler to be called on reconnect.
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    auth : PolymarketWebSocketAuth, optional
        Authentication credentials for USER channel.
    max_subscriptions_per_connection : int, default 200
        The maximum number of subscriptions per WebSocket connection (Polymarket limit is 500).

    References
    ----------
    https://docs.polymarket.com/?python#websocket-api

    """

    def __init__(
        self,
        clock: LiveClock,
        base_url: str | None,
        channel: PolymarketWebSocketChannel,
        handler: Callable[[bytes], None],
        handler_reconnect: Callable[..., Awaitable[None]] | None,
        loop: asyncio.AbstractEventLoop,
        auth: PolymarketWebSocketAuth | None = None,
        max_subscriptions_per_connection: int = 200,
    ) -> None:
        self._clock = clock
        self._log: Logger = Logger(type(self).__name__)

        self._max_subscriptions_per_connection = max_subscriptions_per_connection
        self._channel = channel
        self._base_url: str = base_url or "wss://ws-subscriptions-clob.polymarket.com/ws/"
        self._ws_url = self._base_url + channel.value
        self._auth = auth
        self._handler: Callable[[bytes], None] = handler
        self._handler_reconnect: Callable[..., Awaitable[None]] | None = handler_reconnect
        self._loop = loop
        self._tasks: WeakSet[asyncio.Task] = WeakSet()

        # Multi-client tracking
        self._subscriptions: list[str] = []  # All subscriptions (markets or assets)
        self._subscription_counts: dict[str, int] = {}  # Reference counts per subscription
        self._clients: dict[int, WebSocketClient | None] = {}
        self._client_subscriptions: dict[int, list[str]] = {}
        self._is_connecting: dict[int, bool] = {}
        self._next_client_id: int = 0
        self._lock: asyncio.Lock = asyncio.Lock()

    @property
    def url(self) -> str:
        """
        Return the server URL being used by the client.

        Returns
        -------
        str

        """
        return self._ws_url

    @property
    def subscriptions(self) -> list[str]:
        """
        Return the current active subscriptions for the client.

        Returns
        -------
        list[str]

        """
        return self._subscriptions.copy()

    @property
    def has_subscriptions(self) -> bool:
        """
        Return whether the client has any subscriptions.

        Returns
        -------
        bool

        """
        return bool(self._subscriptions)

    def is_connected(self) -> bool:
        """
        Return whether any client is connected.

        Returns
        -------
        bool

        """
        return any(client is not None and client.is_active() for client in self._clients.values())

    def is_disconnected(self) -> bool:
        """
        Return whether all clients are disconnected.

        Returns
        -------
        bool

        """
        return not self.is_connected()

    def _get_client_for_subscription(self, subscription: str) -> int:
        for client_id, subscriptions in self._client_subscriptions.items():
            if subscription in subscriptions:
                return client_id
        return -1

    def _get_client_id_for_new_subscription(self) -> int:
        for client_id, subs in self._client_subscriptions.items():
            if len(subs) < self._max_subscriptions_per_connection:
                return client_id

        client_id = self._next_client_id
        self._next_client_id += 1
        self._clients[client_id] = None
        self._client_subscriptions[client_id] = []
        self._is_connecting[client_id] = False

        return client_id

    def add_subscription(self, subscription: str) -> None:
        """
        Add a subscription without connecting.

        Use this to queue subscriptions before calling connect().

        Parameters
        ----------
        subscription : str
            The subscription identifier (condition_id for USER, token_id for MARKET).

        """
        count = self._subscription_counts.get(subscription, 0)
        self._subscription_counts[subscription] = count + 1

        if count > 0:
            self._log.debug(f"Already subscribed to {subscription} (count={count + 1})")
            return

        self._subscriptions.append(subscription)

        client_id = self._get_client_id_for_new_subscription()
        self._client_subscriptions[client_id].append(subscription)

    async def subscribe(self, subscription: str) -> None:
        """
        Subscribe to a market or asset.

        If no clients are connected, queues the subscription.
        If clients are connected, subscribes dynamically.
        Uses reference counting - multiple subscribes to the same subscription
        only result in one WebSocket subscription.

        Parameters
        ----------
        subscription : str
            The subscription identifier (condition_id for USER, token_id for MARKET).

        """
        retry_client_id: int | None = None

        async with self._lock:
            count = self._subscription_counts.get(subscription, 0)
            self._subscription_counts[subscription] = count + 1

            if count > 0:
                self._log.debug(f"Already subscribed to {subscription} (count={count + 1})")

                # Check if we need to retry a failed connection
                client_id = self._get_client_for_subscription(subscription)
                if client_id != -1:
                    client = self._clients.get(client_id)
                    is_active = client is not None and client.is_active()
                    is_connecting = self._is_connecting.get(client_id, False)
                    if not is_active and not is_connecting:
                        retry_client_id = client_id

                if retry_client_id is None:
                    return
            else:
                self._subscriptions.append(subscription)
                client_id = self._get_client_id_for_new_subscription()
                self._client_subscriptions[client_id].append(subscription)

        # Retry failed connection (outside lock)
        if retry_client_id is not None:
            self._log.debug(f"ws-client {retry_client_id}: Retrying connection for {subscription}")
            await self._connect_client(retry_client_id)
            return

        # Outside lock to avoid deadlock during connection
        waited_for_connection = False
        while self._is_connecting.get(client_id):
            waited_for_connection = True
            await asyncio.sleep(0.01)

        if client_id not in self._clients or self._clients[client_id] is None:
            await self._connect_client(client_id)
            return

        # Subscription was included in the initial connection message
        if waited_for_connection:
            self._log.debug(f"ws-client {client_id}: {subscription} included in initial connection")
            return

        msg = self._create_dynamic_subscribe_msg(subs=[subscription])
        await self._send(client_id, msg)
        self._log.debug(f"ws-client {client_id}: Subscribed to {subscription}")

    async def unsubscribe(self, subscription: str) -> None:
        """
        Unsubscribe from a market or asset.

        Uses reference counting - only actually unsubscribes from WebSocket
        when all subscribers have unsubscribed.

        Parameters
        ----------
        subscription : str
            The subscription identifier (condition_id for USER, token_id for MARKET).

        """
        async with self._lock:
            count = self._subscription_counts.get(subscription, 0)
            if count <= 0:
                self._log.warning(f"Cannot unsubscribe from {subscription}: not subscribed")
                return

            if count > 1:
                self._subscription_counts[subscription] = count - 1
                self._log.debug(
                    f"Decremented subscription count for {subscription} (count={count - 1})",
                )
                return

            # Count is 1, so this is the last subscriber - actually unsubscribe
            self._subscription_counts.pop(subscription, None)

            if subscription not in self._subscriptions:
                self._log.warning(f"Cannot find subscription {subscription} in subscriptions list")
                return

            client_id = self._get_client_for_subscription(subscription)
            if client_id == -1:
                self._log.warning(f"Cannot find client for subscription {subscription}")
                self._subscriptions.remove(subscription)
                return

            self._subscriptions.remove(subscription)
            if (
                client_id in self._client_subscriptions
                and subscription in self._client_subscriptions[client_id]
            ):
                self._client_subscriptions[client_id].remove(subscription)

            should_disconnect = (
                client_id in self._client_subscriptions
                and not self._client_subscriptions[client_id]
            )

        # Outside lock to avoid deadlock during send
        msg = self._create_dynamic_unsubscribe_msg(subs=[subscription])
        await self._send(client_id, msg)
        self._log.debug(f"ws-client {client_id}: Unsubscribed from {subscription}")

        if should_disconnect:
            await self._disconnect_client(client_id)
            self._log.debug(
                f"ws-client {client_id}: Disconnected due to no remaining subscriptions",
            )

    async def connect(self) -> None:
        """
        Connect websocket clients to the server based on existing subscriptions.
        """
        if not self._subscriptions:
            self._log.error("Cannot connect: no subscriptions")
            return

        for client_id, subs in self._client_subscriptions.items():
            if subs:
                await self._connect_client(client_id)

    async def _connect_client(self, client_id: int) -> None:
        subs = self._client_subscriptions.get(client_id, [])
        if not subs:
            self._log.error(f"ws-client {client_id}: Cannot connect: no subscriptions")
            return

        self._log.debug(f"ws-client {client_id}: Connecting to {self._ws_url}...")
        self._is_connecting[client_id] = True

        try:
            config = WebSocketConfig(
                url=self._ws_url,
                headers=[],
                heartbeat=10,
            )

            self._clients[client_id] = await WebSocketClient.connect(
                loop_=self._loop,
                config=config,
                handler=self._handler,
                post_reconnection=lambda cid=client_id: self._handle_reconnect(cid),
            )

            # Use current tracked subscriptions (may include ones added while connecting)
            current_subs = self._client_subscriptions.get(client_id, [])
            self._log.info(
                f"ws-client {client_id}: Connected to {self._ws_url} with {len(current_subs)} subscriptions",
                LogColor.BLUE,
            )

            await self._subscribe_all(client_id)
        finally:
            self._is_connecting[client_id] = False

    async def _subscribe_all(self, client_id: int) -> None:
        subs = self._client_subscriptions.get(client_id, [])
        if self._channel == PolymarketWebSocketChannel.USER:
            msg = self._create_subscribe_user_channel_msg(markets=subs)
        else:  # MARKET
            msg = self._create_subscribe_market_channel_msg(assets=subs)

        await self._send(client_id, msg)

    def _handle_reconnect(self, client_id: int) -> None:
        if client_id not in self._client_subscriptions or not self._client_subscriptions[client_id]:
            self._log.error(f"ws-client {client_id}: Cannot reconnect: no subscriptions")
            return

        self._log.warning(f"ws-client {client_id}: Reconnected to {self._ws_url}")

        task = self._loop.create_task(self._subscribe_all(client_id))
        self._tasks.add(task)

        if self._handler_reconnect:
            task = self._loop.create_task(self._handler_reconnect())  # type: ignore
            self._tasks.add(task)

    async def disconnect(self) -> None:
        """
        Disconnect all clients from the server.
        """
        await cancel_tasks_with_timeout(self._tasks, self._log)

        tasks = []
        for client_id in list(self._clients.keys()):
            tasks.append(self._disconnect_client(client_id))

        if tasks:
            await asyncio.gather(*tasks)
            self._log.info(f"Disconnected all clients from {self._ws_url}", LogColor.BLUE)

    async def _disconnect_client(self, client_id: int) -> None:
        client = self._clients.get(client_id)
        if client is None:
            return

        # Check state to make this idempotent
        if client.is_disconnecting() or client.is_closed():
            self._log.debug(f"ws-client {client_id}: Already disconnecting/closed, skipping")
            return

        self._log.debug(f"ws-client {client_id}: Disconnecting...")
        try:
            await client.disconnect()
        except WebSocketClientError as e:
            self._log.error(f"ws-client {client_id}: {e!s}")

        self._clients[client_id] = None

    def _create_subscribe_market_channel_msg(self, assets: list[str]) -> dict[str, Any]:
        return {
            "type": "market",
            "assets_ids": assets,
        }

    def _create_subscribe_user_channel_msg(self, markets: list[str]) -> dict[str, Any]:
        return {
            "auth": self._auth,
            "type": "user",
            "markets": markets,
        }

    def _create_dynamic_subscribe_msg(self, subs: list[str]) -> dict[str, Any]:
        if self._channel == PolymarketWebSocketChannel.USER:
            return {
                "auth": self._auth,
                "markets": subs,
                "operation": "subscribe",
            }
        else:  # MARKET
            return {
                "assets_ids": subs,
                "operation": "subscribe",
            }

    def _create_dynamic_unsubscribe_msg(self, subs: list[str]) -> dict[str, Any]:
        if self._channel == PolymarketWebSocketChannel.USER:
            return {
                "markets": subs,
                "operation": "unsubscribe",
            }
        else:  # MARKET
            return {
                "assets_ids": subs,
                "operation": "unsubscribe",
            }

    async def _send(self, client_id: int, msg: dict[str, Any]) -> None:
        client = self._clients.get(client_id)
        if client is None:
            self._log.error(f"ws-client {client_id}: Cannot send message {msg}: not connected")
            return

        self._log.debug(f"ws-client {client_id}: SENDING: {msg}")

        try:
            await client.send_text(msgspec.json.encode(msg))
        except WebSocketClientError as e:
            self._log.error(f"ws-client {client_id}: {e!s}")

    # Legacy compatibility methods (deprecated, for backwards compatibility)

    def subscribe_market(self, condition_id: str) -> None:
        """
        Add a market subscription (legacy method).

        .. deprecated::
            Use `add_subscription()` or `subscribe()` instead.

        """
        self.add_subscription(condition_id)

    def subscribe_book(self, asset: str) -> None:
        """
        Add an asset subscription (legacy method).

        .. deprecated::
            Use `add_subscription()` or `subscribe()` instead.

        """
        self.add_subscription(asset)

    def market_subscriptions(self) -> list[str]:
        """
        Return market subscriptions (legacy method).

        .. deprecated::
            Use `subscriptions` property instead.

        """
        if self._channel == PolymarketWebSocketChannel.USER:
            return self._subscriptions.copy()
        return []

    def asset_subscriptions(self) -> list[str]:
        """
        Return asset subscriptions (legacy method).

        .. deprecated::
            Use `subscriptions` property instead.

        """
        if self._channel == PolymarketWebSocketChannel.MARKET:
            return self._subscriptions.copy()
        return []
