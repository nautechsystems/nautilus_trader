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
from collections.abc import Awaitable
from collections.abc import Callable
from typing import Any
from weakref import WeakSet

import msgspec

from nautilus_trader.adapters.binance.common.symbol import BinanceSymbol
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.nautilus_pyo3 import WebSocketClient
from nautilus_trader.core.nautilus_pyo3 import WebSocketClientError
from nautilus_trader.core.nautilus_pyo3 import WebSocketConfig
from nautilus_trader.live.cancellation import cancel_tasks_with_timeout


class BinanceWebSocketClient:
    """
    Provides a Binance streaming WebSocket client.

    Manages multiple WebSocket connections with up to 200 subscriptions per connection
    as per Binance documentation.

    Parameters
    ----------
    clock : LiveClock
        The clock for the client.
    base_url : str
        The base URL for the WebSocket connection.
    handler : Callable[[bytes], None]
        The callback handler for message events.
    handler_reconnect : Callable[..., Awaitable[None]], optional
        The callback handler to be called on reconnect.
    loop : asyncio.AbstractEventLoop
        The event loop for the client.

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#websocket-market-streams

    """

    MAX_SUBSCRIPTIONS_PER_CLIENT = 200
    MAX_CLIENTS = 20  # Allows up to 4000 total subscriptions (20 x 200)

    def __init__(
        self,
        clock: LiveClock,
        base_url: str,
        handler: Callable[[bytes], None],
        handler_reconnect: Callable[..., Awaitable[None]] | None,
        loop: asyncio.AbstractEventLoop,
    ) -> None:
        self._clock = clock
        self._log: Logger = Logger(type(self).__name__)

        self._base_url: str = base_url
        self._handler: Callable[[bytes], None] = handler
        self._handler_reconnect: Callable[..., Awaitable[None]] | None = handler_reconnect
        self._loop = loop
        self._tasks: WeakSet[asyncio.Task] = WeakSet()

        self._streams: list[str] = []
        self._clients: dict[int, WebSocketClient | None] = {}  # Client ID -> WebSocket client
        self._client_streams: dict[int, list[str]] = {}  # Client ID -> streams
        self._is_connecting: dict[int, bool] = {}  # Client ID -> is_connecting flag
        self._msg_id: int = 0
        self._next_client_id: int = 0

    @property
    def url(self) -> str:
        """
        Return the server URL being used by the client.

        Returns
        -------
        str

        """
        return self._base_url

    @property
    def subscriptions(self) -> list[str]:
        """
        Return the current active subscriptions for the client.

        Returns
        -------
        str

        """
        return self._streams.copy()

    @property
    def has_subscriptions(self) -> bool:
        """
        Return whether the client has subscriptions.

        Returns
        -------
        bool

        """
        return bool(self._streams)

    def _get_client_for_stream(self, stream: str) -> int:
        """
        Determine which client is handling a particular stream.

        Returns
        -------
        int
            The client ID handling the stream, or -1 if not found.

        """
        for client_id, streams in self._client_streams.items():
            if stream in streams:
                return client_id
        return -1

    def _get_client_id_for_new_subscription(self) -> int:
        """
        Find or create a client ID for a new subscription.

        Returns
        -------
        int
            The client ID to use for the new subscription.

        Raises
        ------
        RuntimeError
            If maximum number of clients and subscriptions are exceeded.

        """
        # Try to find an existing client with room for another subscription
        for client_id, streams in self._client_streams.items():
            if len(streams) < self.MAX_SUBSCRIPTIONS_PER_CLIENT:
                return client_id

        # Check if we can create a new client
        if len(self._clients) >= self.MAX_CLIENTS:
            max_total_streams = self.MAX_CLIENTS * self.MAX_SUBSCRIPTIONS_PER_CLIENT
            raise RuntimeError(
                f"Cannot create new subscription: maximum limit of {max_total_streams} "
                f"total subscriptions ({self.MAX_CLIENTS} clients x "
                f"{self.MAX_SUBSCRIPTIONS_PER_CLIENT} subscriptions) exceeded",
            )

        # Create a new client ID
        client_id = self._next_client_id
        self._next_client_id += 1
        self._clients[client_id] = None
        self._client_streams[client_id] = []
        self._is_connecting[client_id] = False

        return client_id

    async def connect(self) -> None:
        """
        Connect websocket clients to the server based on existing subscriptions.
        """
        if not self._streams:
            self._log.error("Cannot connect: no streams for initial connection")
            return

        # Group streams by client (using existing assignments or creating new ones)
        client_streams: dict[int, list[str]] = {}
        for stream in self._streams:
            client_id = self._get_client_for_stream(stream)
            if client_id == -1:
                client_id = self._get_client_id_for_new_subscription()

            if client_id not in client_streams:
                client_streams[client_id] = []
            client_streams[client_id].append(stream)

        # Connect clients
        for client_id, streams in client_streams.items():
            await self._connect_client(client_id, streams)

    async def _connect_client(self, client_id: int, streams: list[str]) -> None:
        """
        Connect a single websocket client to the server.

        Parameters
        ----------
        client_id : int
            ID of the client to connect
        streams : list[str]
            List of streams for this client

        """
        if not streams:
            self._log.error(f"Cannot connect client {client_id}: no streams provided")
            return

        # Update client streams tracking
        self._client_streams[client_id] = streams.copy()

        # Binance expects at least one stream for the initial connection
        initial_stream = streams[0]
        ws_url = self._base_url + f"/stream?streams={initial_stream}"

        self._log.debug(f"ws-client {client_id}: Connecting to {ws_url}...")
        self._is_connecting[client_id] = True

        config = WebSocketConfig(
            url=ws_url,
            handler=self._handler,
            heartbeat=60,
            headers=[],
            ping_handler=lambda raw: self._handle_ping(client_id, raw),
        )

        self._clients[client_id] = await WebSocketClient.connect(
            config=config,
            post_reconnection=lambda: self._handle_reconnect(client_id),
        )
        self._is_connecting[client_id] = False
        self._log.info(f"ws-client {client_id}: Connected to {self._base_url}", LogColor.BLUE)
        self._log.debug(f"ws-client {client_id}: Subscribed to {initial_stream}")

        # If there are multiple streams, subscribe to the rest
        if len(streams) > 1:
            msg = self._create_subscribe_msg(streams=streams[1:])
            await self._send(client_id, msg)
            self._log.debug(
                f"ws-client {client_id}: Subscribed to additional {len(streams) - 1} streams",
            )

    def _handle_ping(self, client_id: int, raw: bytes) -> None:
        task = self._loop.create_task(self.send_pong(client_id, raw))
        self._tasks.add(task)

    async def send_pong(self, client_id: int, raw: bytes) -> None:
        """
        Send the given raw payload to the server as a PONG message.
        """
        client = self._clients.get(client_id)
        if client is None:
            return

        try:
            await client.send_pong(raw)
        except WebSocketClientError as e:
            self._log.error(f"ws-client {client_id}: {e!s}")

    def _handle_reconnect(self, client_id: int) -> None:
        """
        Handle reconnection for a specific client.
        """
        if client_id not in self._client_streams or not self._client_streams[client_id]:
            self._log.error(f"ws-client {client_id}: Cannot reconnect: no streams for this client")
            return

        self._log.warning(f"ws-client {client_id}: Reconnected to {self._base_url}")

        # Re-subscribe to all streams for this client
        streams = self._client_streams[client_id]
        task = self._loop.create_task(self._resubscribe_client(client_id, streams))
        self._tasks.add(task)

        if self._handler_reconnect:
            task = self._loop.create_task(self._handler_reconnect())  # type: ignore
            self._tasks.add(task)

    async def _resubscribe_client(self, client_id: int, streams: list[str]) -> None:
        """
        Resubscribe all streams for a given client.
        """
        if not streams:
            return

        msg = self._create_subscribe_msg(streams=streams)
        await self._send(client_id, msg)
        self._log.debug(f"ws-client {client_id}: Resubscribed to {len(streams)} streams")

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
            self._log.info(f"Disconnected all clients from {self._base_url}", LogColor.BLUE)

    async def _disconnect_client(self, client_id: int) -> None:
        """
        Disconnect a specific client from the server.
        """
        client = self._clients.get(client_id)
        if client is None:
            return

        # Check Rust-level state to make this idempotent
        if client.is_disconnecting() or client.is_closed():
            self._log.debug(f"ws-client {client_id}: Already disconnecting/closed, skipping")
            return

        self._log.debug(f"ws-client {client_id}: Disconnecting...")
        try:
            await client.disconnect()
        except WebSocketClientError as e:
            self._log.error(f"ws-client {client_id}: {e!s}")

        self._clients[client_id] = None  # Dispose (will go out of scope)
        self._log.debug(f"ws-client {client_id}: Disconnected from {self._base_url}")

    async def subscribe_listen_key(self, listen_key: str) -> None:
        """
        Subscribe to user data stream.
        """
        await self._subscribe(listen_key)

    async def unsubscribe_listen_key(self, listen_key: str) -> None:
        """
        Unsubscribe from user data stream.
        """
        await self._unsubscribe(listen_key)

    async def subscribe_agg_trades(self, symbol: str) -> None:
        """
        Subscribe to aggregate trade stream.

        The Aggregate Trade Streams push trade information that is aggregated for a single taker order.
        Stream Name: <symbol>@aggTrade
        Update Speed: Real-time

        """
        stream = f"{BinanceSymbol(symbol).lower()}@aggTrade"
        await self._subscribe(stream)

    async def unsubscribe_agg_trades(self, symbol: str) -> None:
        """
        Unsubscribe from aggregate trade stream.
        """
        stream = f"{BinanceSymbol(symbol).lower()}@aggTrade"
        await self._unsubscribe(stream)

    async def subscribe_trades(self, symbol: str) -> None:
        """
        Subscribe to trade stream.

        The Trade Streams push raw trade information; each trade has a unique buyer and seller.
        Stream Name: <symbol>@trade
        Update Speed: Real-time

        """
        stream = f"{BinanceSymbol(symbol).lower()}@trade"
        await self._subscribe(stream)

    async def unsubscribe_trades(self, symbol: str) -> None:
        """
        Unsubscribe from trade stream.
        """
        stream = f"{BinanceSymbol(symbol).lower()}@trade"
        await self._unsubscribe(stream)

    async def subscribe_bars(
        self,
        symbol: str,
        interval: str,
    ) -> None:
        """
        Subscribe to bar (kline/candlestick) stream.

        The Kline/Candlestick Stream push updates to the current klines/candlestick every second.
        Stream Name: <symbol>@kline_<interval>
        interval:
        m -> minutes; h -> hours; d -> days; w -> weeks; M -> months
        - 1m
        - 3m
        - 5m
        - 15m
        - 30m
        - 1h
        - 2h
        - 4h
        - 6h
        - 8h
        - 12h
        - 1d
        - 3d
        - 1w
        - 1M
        Update Speed: 2000ms

        """
        stream = f"{BinanceSymbol(symbol).lower()}@kline_{interval}"
        await self._subscribe(stream)

    async def unsubscribe_bars(
        self,
        symbol: str,
        interval: str,
    ) -> None:
        """
        Unsubscribe from bar (kline/candlestick) stream.
        """
        stream = f"{BinanceSymbol(symbol).lower()}@kline_{interval}"
        await self._unsubscribe(stream)

    async def subscribe_mini_ticker(
        self,
        symbol: str | None = None,
    ) -> None:
        """
        Subscribe to individual symbol or all symbols mini ticker stream.

        24hr rolling window mini-ticker statistics.
        These are NOT the statistics of the UTC day, but a 24hr rolling window for the previous 24hrs
        Stream Name: <symbol>@miniTicker or
        Stream Name: !miniTicker@arr
        Update Speed: 1000ms

        """
        if symbol is None:
            stream = "!miniTicker@arr"
        else:
            stream = f"{BinanceSymbol(symbol).lower()}@miniTicker"
        await self._subscribe(stream)

    async def unsubscribe_mini_ticker(
        self,
        symbol: str | None = None,
    ) -> None:
        """
        Unsubscribe to individual symbol or all symbols mini ticker stream.
        """
        if symbol is None:
            stream = "!miniTicker@arr"
        else:
            stream = f"{BinanceSymbol(symbol).lower()}@miniTicker"
        await self._unsubscribe(stream)

    async def subscribe_ticker(
        self,
        symbol: str | None = None,
    ) -> None:
        """
        Subscribe to individual symbol or all symbols ticker stream.

        24hr rolling window ticker statistics for a single symbol.
        These are NOT the statistics of the UTC day, but a 24hr rolling window for the previous 24hrs.
        Stream Name: <symbol>@ticker or
        Stream Name: !ticker@arr
        Update Speed: 1000ms

        """
        if symbol is None:
            stream = "!ticker@arr"
        else:
            stream = f"{BinanceSymbol(symbol).lower()}@ticker"
        await self._subscribe(stream)

    async def unsubscribe_ticker(
        self,
        symbol: str | None = None,
    ) -> None:
        """
        Unsubscribe from individual symbol or all symbols ticker stream.
        """
        if symbol is None:
            stream = "!ticker@arr"
        else:
            stream = f"{BinanceSymbol(symbol).lower()}@ticker"
        await self._unsubscribe(stream)

    async def subscribe_book_ticker(
        self,
        symbol: str | None = None,
    ) -> None:
        """
        Subscribe to individual symbol or all book tickers stream.

        Pushes any update to the best bid or ask's price or quantity in real-time for a specified symbol.
        Stream Name: <symbol>@bookTicker or
        Stream Name: !bookTicker
        Update Speed: realtime

        """
        if symbol is None:
            stream = "!bookTicker"
        else:
            stream = f"{BinanceSymbol(symbol).lower()}@bookTicker"
        await self._subscribe(stream)

    async def unsubscribe_book_ticker(
        self,
        symbol: str | None = None,
    ) -> None:
        """
        Unsubscribe from individual symbol or all book tickers.
        """
        if symbol is None:
            stream = "!bookTicker"
        else:
            stream = f"{BinanceSymbol(symbol).lower()}@bookTicker"
        await self._unsubscribe(stream)

    async def subscribe_partial_book_depth(
        self,
        symbol: str,
        depth: int,
        speed: int,
    ) -> None:
        """
        Subscribe to partial book depth stream.

        Top bids and asks, Valid are 5, 10, or 20.
        Stream Names: <symbol>@depth<levels> OR <symbol>@depth<levels>@100ms.
        Update Speed: 1000ms or 100ms

        """
        stream = f"{BinanceSymbol(symbol).lower()}@depth{depth}@{speed}ms"
        await self._subscribe(stream)

    async def unsubscribe_partial_book_depth(
        self,
        symbol: str,
        depth: int,
        speed: int,
    ) -> None:
        """
        Unsubscribe from partial book depth stream.
        """
        stream = f"{BinanceSymbol(symbol).lower()}@depth{depth}@{speed}ms"
        await self._unsubscribe(stream)

    async def subscribe_diff_book_depth(
        self,
        symbol: str,
        speed: int,
    ) -> None:
        """
        Subscribe to diff book depth stream.

        Stream Name: <symbol>@depth OR <symbol>@depth@100ms
        Update Speed: 1000ms or 100ms
        Order book price and quantity depth updates used to locally manage an order book.

        """
        stream = f"{BinanceSymbol(symbol).lower()}@depth@{speed}ms"
        await self._subscribe(stream)

    async def unsubscribe_diff_book_depth(
        self,
        symbol: str,
        speed: int,
    ) -> None:
        """
        Unsubscribe from diff book depth stream.
        """
        stream = f"{BinanceSymbol(symbol).lower()}@depth@{speed}ms"
        await self._unsubscribe(stream)

    async def subscribe_mark_price(
        self,
        symbol: str | None = None,
        speed: int | None = None,
    ) -> None:
        """
        Subscribe to aggregate mark price stream.
        """
        if speed and speed not in (1000, 3000):
            raise ValueError(f"`speed` options are 1000ms or 3000ms only, was {speed}")

        if symbol is None:
            stream = "!markPrice@arr"
        else:
            stream = f"{BinanceSymbol(symbol).lower()}@markPrice"

        if speed:
            stream += f"@{int(speed / 1000)}s"

        await self._subscribe(stream)

    async def unsubscribe_mark_price(
        self,
        symbol: str | None = None,
        speed: int | None = None,
    ) -> None:
        """
        Unsubscribe from aggregate mark price stream.
        """
        if speed not in (1000, 3000):
            raise ValueError(f"`speed` options are 1000ms or 3000ms only, was {speed}")
        if symbol is None:
            stream = "!markPrice@arr"
        else:
            stream = f"{BinanceSymbol(symbol).lower()}@markPrice@{int(speed / 1000)}s"
        await self._unsubscribe(stream)

    async def _subscribe(self, stream: str) -> None:
        if stream in self._streams:
            self._log.warning(f"Cannot subscribe to {stream}: already subscribed")
            return  # Already subscribed

        self._streams.append(stream)

        # Determine which client should handle this stream
        client_id = self._get_client_id_for_new_subscription()

        # Add to client's stream list
        if client_id not in self._client_streams:
            self._client_streams[client_id] = []
        self._client_streams[client_id].append(stream)

        # Wait for client to finish connecting if it's in progress
        while self._is_connecting.get(client_id):
            await asyncio.sleep(0.01)

        # If client doesn't exist yet, connect it
        if client_id not in self._clients or self._clients[client_id] is None:
            await self._connect_client(client_id, [stream])
            return

        # Otherwise, send subscription message to existing client
        msg = self._create_subscribe_msg(streams=[stream])
        await self._send(client_id, msg)
        self._log.debug(f"ws-client {client_id}: Subscribed to {stream}")

    async def _unsubscribe(self, stream: str) -> None:
        if stream not in self._streams:
            self._log.warning(f"Cannot unsubscribe from {stream}: not subscribed")
            return  # Not subscribed

        # Find which client has this stream
        client_id = self._get_client_for_stream(stream)
        if client_id == -1:
            self._log.warning(f"Cannot find client for stream {stream}")
            self._streams.remove(stream)
            return

        # Remove from global streams list
        self._streams.remove(stream)

        # Remove from client's streams list
        if client_id in self._client_streams:
            if stream in self._client_streams[client_id]:
                self._client_streams[client_id].remove(stream)

        # Send unsubscribe message
        msg = self._create_unsubscribe_msg(streams=[stream])
        await self._send(client_id, msg)
        self._log.debug(f"ws-client {client_id}: Unsubscribed from {stream}")

        # If client has no more streams, disconnect it
        if client_id in self._client_streams and not self._client_streams[client_id]:
            await self._disconnect_client(client_id)
            self._log.debug(
                f"ws-client {client_id}: Disconnected due to no remaining subscriptions",
            )

    def _create_subscribe_msg(self, streams: list[str]) -> dict[str, Any]:
        message = {
            "method": "SUBSCRIBE",
            "params": streams,
            "id": self._msg_id,
        }
        self._msg_id += 1
        return message

    def _create_unsubscribe_msg(self, streams: list[str]) -> dict[str, Any]:
        message = {
            "method": "UNSUBSCRIBE",
            "params": streams,
            "id": self._msg_id,
        }
        self._msg_id += 1
        return message

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
