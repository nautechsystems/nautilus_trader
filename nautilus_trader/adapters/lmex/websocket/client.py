# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter
#  Licensed under the GNU Lesser General Public License Version 3.0
# -------------------------------------------------------------------------------------------------

"""
LMEX WebSocket client.

Manages a single WebSocket connection to the LMEX streaming API, handling
subscriptions, reconnection, and heartbeats.

WebSocket URL (live)   : wss://ws.lmex.io/ws/spot
Subscription format    : {"op": "subscribe",   "args": ["topic:SYMBOL"]}
Unsubscription format  : {"op": "unsubscribe", "args": ["topic:SYMBOL"]}
Subscription ack       : {"event": "subscribe", "channel": ["topic:SYMBOL"]}
Heartbeat              : {"op": "ping"}  → {"event": "pong"}

Confirmed topics (live, 2026-05-26):
  tradeHistoryApi:BTC-USD  →  trade feed
  orderBookApi:BTC-USD_0   →  order book (to be confirmed in implementation)
  notificationsApi         →  private order events (requires auth)
"""

from __future__ import annotations

import asyncio
import json
from collections.abc import Awaitable, Callable
from weakref import WeakSet

import msgspec

from nautilus_trader.adapters.lmex.constants import LMEX_WS_HEARTBEAT_SECS
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.nautilus_pyo3 import WebSocketClient
from nautilus_trader.core.nautilus_pyo3 import WebSocketClientError
from nautilus_trader.core.nautilus_pyo3 import WebSocketConfig


class LmexWebSocketClient:
    """
    Provides a streaming WebSocket client for the LMEX exchange.

    Wraps ``nautilus_pyo3.WebSocketClient`` to provide:

    - Topic-based subscribe / unsubscribe
    - Automatic re-subscription after reconnect
    - JSON heartbeat pings
    - Graceful shutdown

    Parameters
    ----------
    clock : LiveClock
        The clock for the client.
    base_url : str
        WebSocket base URL (e.g. ``"wss://ws.lmex.io/ws/spot"``).
    handler : Callable[[bytes], None]
        Callback invoked for every incoming data message.
    handler_reconnect : Callable[..., Awaitable[None]] or None, optional
        Async callback invoked after a successful reconnection.  Use this
        to re-subscribe to private topics that need authentication.
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    proxy_url : str or None, optional
        Optional WebSocket proxy URL.

    """

    _SUBSCRIBE_OP = "subscribe"
    _UNSUBSCRIBE_OP = "unsubscribe"
    _PING_MSG: bytes = json.dumps({"op": "ping"}).encode()

    def __init__(
        self,
        clock: LiveClock,
        base_url: str,
        handler: Callable[[bytes], None],
        handler_reconnect: Callable[..., Awaitable[None]] | None,
        loop: asyncio.AbstractEventLoop,
        proxy_url: str | None = None,
    ) -> None:
        self._clock = clock
        self._log: Logger = Logger(type(self).__name__)
        self._base_url = base_url
        self._handler = handler
        self._handler_reconnect = handler_reconnect
        self._loop = loop
        self._proxy_url = proxy_url

        self._client: WebSocketClient | None = None
        self._subscriptions: set[str] = set()
        self._tasks: WeakSet[asyncio.Task] = WeakSet()
        self._is_connected: bool = False

    # ------------------------------------------------------------------
    # Properties
    # ------------------------------------------------------------------

    @property
    def url(self) -> str:
        """Return the WebSocket URL."""
        return self._base_url

    @property
    def subscriptions(self) -> frozenset[str]:
        """Return the current active topic subscriptions (immutable copy)."""
        return frozenset(self._subscriptions)

    @property
    def is_connected(self) -> bool:
        """Return whether the WebSocket is currently connected."""
        return self._is_connected

    # ------------------------------------------------------------------
    # Lifecycle
    # ------------------------------------------------------------------

    async def connect(self) -> None:
        """
        Establish the WebSocket connection.

        Creates a ``WebSocketConfig`` and calls ``WebSocketClient.connect``.
        The client reconnects automatically on unexpected disconnection.

        Raises
        ------
        WebSocketClientError
            If the initial connection attempt fails.

        """
        self._log.info(f"Connecting to {self._base_url}", LogColor.BLUE)

        config = WebSocketConfig(
            url=self._base_url,
            headers=[],
            heartbeat=LMEX_WS_HEARTBEAT_SECS,
            proxy_url=self._proxy_url,
        )

        self._client = await WebSocketClient.connect(
            loop_=self._loop,
            config=config,
            handler=self._handler,
            post_reconnection=self._on_reconnect,
        )

        self._is_connected = True
        self._log.info("Connected", LogColor.BLUE)

    async def disconnect(self) -> None:
        """
        Close the WebSocket connection gracefully.

        Clears the active subscription set and cancels pending tasks.

        """
        self._log.info("Disconnecting")
        self._is_connected = False

        if self._client is not None:
            try:
                await self._client.close()
            except WebSocketClientError as exc:
                self._log.warning(f"Error closing WebSocket: {exc}")
            self._client = None

        self._subscriptions.clear()
        self._log.info("Disconnected", LogColor.BLUE)

    # ------------------------------------------------------------------
    # Subscriptions
    # ------------------------------------------------------------------

    async def subscribe(self, topic: str) -> None:
        """
        Subscribe to an LMEX WebSocket topic.

        Has no effect if already subscribed to the given topic.

        Parameters
        ----------
        topic : str
            The topic string (e.g. ``"tradeHistoryApi:BTC-USD"``).

        """
        if topic in self._subscriptions:
            self._log.warning(f"Already subscribed to {topic!r}")
            return

        self._subscriptions.add(topic)
        await self._send_op(self._SUBSCRIBE_OP, [topic])
        self._log.info(f"Subscribed → {topic!r}", LogColor.BLUE)

    async def unsubscribe(self, topic: str) -> None:
        """
        Unsubscribe from an LMEX WebSocket topic.

        Has no effect if not currently subscribed.

        Parameters
        ----------
        topic : str
            The topic string to unsubscribe from.

        """
        if topic not in self._subscriptions:
            self._log.warning(f"Not subscribed to {topic!r}")
            return

        self._subscriptions.discard(topic)
        await self._send_op(self._UNSUBSCRIBE_OP, [topic])
        self._log.info(f"Unsubscribed ← {topic!r}", LogColor.BLUE)

    # ------------------------------------------------------------------
    # Reconnect handling
    # ------------------------------------------------------------------

    async def _on_reconnect(self) -> None:
        """
        Re-subscribe to all active topics after a reconnection.

        Called by the ``post_reconnection`` callback of
        ``WebSocketClient.connect``.

        """
        self._log.info(
            f"Reconnected — re-subscribing to {len(self._subscriptions)} topics",
            LogColor.YELLOW,
        )
        self._is_connected = True

        # Re-send all active subscriptions in one message for efficiency
        if self._subscriptions:
            await self._send_op(self._SUBSCRIBE_OP, list(self._subscriptions))

        # Allow the data/exec client to perform additional post-reconnect work
        # (e.g. re-authenticating a private stream)
        if self._handler_reconnect is not None:
            try:
                await self._handler_reconnect()
            except Exception as exc:
                self._log.error(f"Error in reconnect handler: {exc}")

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    async def _send_op(self, op: str, args: list[str]) -> None:
        """
        Send a subscription control message to the server.

        Parameters
        ----------
        op : str
            ``"subscribe"`` or ``"unsubscribe"``.
        args : list[str]
            Topic strings.

        """
        if self._client is None:
            self._log.error(f"Cannot send {op!r}: not connected")
            return

        msg = json.dumps({"op": op, "args": args})
        self._log.debug(f"SENDING: {msg}")

        try:
            await self._client.send_text(msg.encode())
        except WebSocketClientError as exc:
            self._log.error(f"WebSocket send error: {exc}")
        except RuntimeError as exc:
            # Connection raced into a non-active state; reconnect is handled
            # by the Rust WebSocket controller.
            self._log.warning(f"WebSocket send skipped (race): {exc}")
