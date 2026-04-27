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
"""
Binance WebSocket API client for user data streams.

This client uses the authenticated WebSocket API endpoint with
`session.logon`. Supports both Ed25519 and HMAC API keys.

Spot uses `userDataStream.subscribe` — events arrive inline on the same connection.
Futures + Ed25519 uses `userDataStream.start` via WS API — events are delivered on
a separate stream connection at `{stream_base_url}/ws?listenKey={listenKey}`.
Futures + HMAC uses REST API for listenKey management (Binance Futures WS API
`session.logon` only accepts Ed25519).

"""

import asyncio
from collections.abc import Awaitable
from collections.abc import Callable
from typing import Any

import msgspec

from nautilus_trader.adapters.binance.common.credentials import extract_ed25519_private_key
from nautilus_trader.adapters.binance.common.credentials import is_ed25519_private_key
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.user import BinanceUserDataHttpAPI
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.nautilus_pyo3 import WebSocketClient
from nautilus_trader.core.nautilus_pyo3 import WebSocketClientError
from nautilus_trader.core.nautilus_pyo3 import WebSocketConfig
from nautilus_trader.core.nautilus_pyo3 import ed25519_signature
from nautilus_trader.core.nautilus_pyo3 import hmac_signature
from nautilus_trader.core.nautilus_pyo3 import mask_api_key


class BinanceUserDataWebSocketClient:
    """
    Provides a Binance WebSocket API client for user data streams.

    Uses the new authenticated WebSocket API endpoint with `session.logon`
    instead of the deprecated listenKey REST API.

    Supports both Ed25519 and HMAC API keys. The key type is auto-detected
    from the api_secret format.

    Parameters
    ----------
    clock : LiveClock
        The clock for the client.
    base_url : str
        The base URL for the WebSocket API connection.
    handler : Callable[[bytes], None]
        The callback handler for message events.
    api_key : str
        The Binance API key.
    api_secret : str
        The Binance API secret (HMAC string or Ed25519 base64/PEM key).
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    is_futures : bool, default False
        If True, uses Futures WebSocket API methods (userDataStream.start/stop/ping)
        and connects a separate stream for event delivery.
        If False, uses Spot WebSocket API methods (userDataStream.subscribe/unsubscribe).
    stream_base_url : str, optional
        The base URL for the futures private stream (e.g. wss://fstream.binance.com/private).
        Required when `is_futures` is True.
    is_ed25519 : bool, optional
        Force Ed25519 signing when True. When None (default), auto-detects
        from the api_secret format (PEM header or PKCS#8 OID).
    http_client : BinanceHttpClient, optional
        HTTP client for REST listenKey management. Required for HMAC Futures
        (Binance Futures WS API `session.logon` only accepts Ed25519).
    account_type : BinanceAccountType, optional
        The account type, required when `http_client` is provided.
    on_resubscribe : Callable[[], Awaitable[None]], optional
        Async callback invoked after a successful resubscribe or reconnect.
        Used to trigger execution reconciliation to close any event-loss gap
        during listenKey rotation.
    proxy_url : str, optional
        The proxy URL for the WebSocket connection.

    """

    _KEEPALIVE_INTERVAL_SECS = 1800  # 30 minutes
    _MAX_KEEPALIVE_FAILURES = 1

    def __init__(
        self,
        clock: LiveClock,
        base_url: str,
        handler: Callable[[bytes], None],
        api_key: str,
        api_secret: str,
        loop: asyncio.AbstractEventLoop,
        is_futures: bool = False,
        stream_base_url: str | None = None,
        is_ed25519: bool | None = None,
        http_client: BinanceHttpClient | None = None,
        account_type: BinanceAccountType | None = None,
        on_resubscribe: Callable[[], Awaitable[None]] | None = None,
        proxy_url: str | None = None,
    ) -> None:
        self._clock = clock
        self._log: Logger = Logger(type(self).__name__)

        self._base_url: str = base_url
        self._handler: Callable[[bytes], None] = handler
        self._loop = loop
        self._is_futures: bool = is_futures
        self._stream_base_url: str | None = stream_base_url
        self._proxy_url: str | None = proxy_url

        self._api_key: str = api_key

        # Resolve key type: honor explicit flag, otherwise auto-detect
        use_ed25519 = is_ed25519 if is_ed25519 is not None else is_ed25519_private_key(api_secret)

        if use_ed25519:
            self._ed25519_key: bytes | None = extract_ed25519_private_key(api_secret)
            self._hmac_secret: str | None = None
        else:
            self._ed25519_key = None
            self._hmac_secret = api_secret

        # REST listenKey API for HMAC Futures
        # (Binance Futures WS API session.logon only accepts Ed25519)
        if http_client is not None and account_type is not None:
            self._http_user: BinanceUserDataHttpAPI | None = BinanceUserDataHttpAPI(
                client=http_client,
                account_type=account_type,
            )
        else:
            self._http_user = None

        self._on_resubscribe = on_resubscribe

        # WebSocket API connection (auth + requests)
        self._client: WebSocketClient | None = None
        self._msg_id: int = 0
        self._pending_requests: dict[str, asyncio.Future[dict[str, Any]]] = {}
        self._subscription_id: str | None = None
        self._is_authenticated: bool = False
        self._is_recovery_failed: bool = False
        self._reconnect_task: asyncio.Task | None = None
        self._keepalive_task: asyncio.Task | None = None
        self._recovery_lock: asyncio.Lock = asyncio.Lock()

        # When paused, _handle_stream_message queues raw bytes into the
        # buffer instead of dispatching. The recovery path uses this to hold
        # fresh stream events while the mass-status snapshot is emitted, so
        # a later snapshot cannot replay stale state over newer events.
        self._dispatch_paused: bool = False
        self._dispatch_buffer: list[bytes] = []

        # Futures stream connection (event delivery)
        self._stream_client: WebSocketClient | None = None

    @property
    def subscription_id(self) -> str | None:
        """
        Return the current user data stream subscription ID.
        """
        return self._subscription_id

    @property
    def is_authenticated(self) -> bool:
        """
        Return whether the session is authenticated.
        """
        return self._is_authenticated

    @property
    def _use_rest_listen_key(self) -> bool:
        return self._is_futures and self._http_user is not None

    def _get_sign(self, data: str) -> str:
        if self._ed25519_key is not None:
            return ed25519_signature(self._ed25519_key, data)
        return hmac_signature(self._hmac_secret, data)

    def _next_msg_id(self) -> str:
        msg_id = str(self._msg_id)
        self._msg_id += 1
        return msg_id

    def _handle_message(self, raw: bytes) -> None:
        try:
            msg = msgspec.json.decode(raw)
        except msgspec.DecodeError:
            self._log.error(f"Failed to decode message: {raw!r}")
            return

        # Normalize to string since Binance may return numeric IDs
        msg_id = msg.get("id")
        if msg_id is not None:
            msg_id_str = str(msg_id)
            if msg_id_str in self._pending_requests:
                future = self._pending_requests.pop(msg_id_str)
                if not future.done():
                    future.set_result(msg)
                return

        # Handle Spot stream termination by resubscribing
        if b'"eventStreamTerminated"' in raw:
            self._log.warning("Received eventStreamTerminated, resubscribing...")
            self._loop.create_task(self._resubscribe())
            return

        # Spot wraps events in {"subscriptionId": N, "event": {...}}
        event = msg.get("event")
        if event is not None:
            payload = msgspec.json.encode(event)

            if self._dispatch_paused:
                self._dispatch_buffer.append(payload)
                return

            self._handler(payload)
            return

        self._log.warning(f"Unhandled WebSocket API message: {raw!r}")

    def _handle_stream_message(self, raw: bytes) -> None:
        if b'"listenKeyExpired"' in raw:
            self._log.warning("Listen key expired, resubscribing...")
            self._loop.create_task(self._resubscribe())
            return

        if self._dispatch_paused:
            self._dispatch_buffer.append(raw)
            return

        self._handler(raw)

    async def connect(self) -> None:
        """
        Connect to the WebSocket API server.
        """
        self._is_recovery_failed = False

        if self._use_rest_listen_key:
            self._log.info("Using REST listenKey mode (HMAC Futures)", LogColor.BLUE)
            return

        self._log.debug(f"Connecting to {self._base_url}...")

        config = WebSocketConfig(
            url=self._base_url,
            headers=[("X-MBX-APIKEY", self._api_key)],
            heartbeat=60,
            proxy_url=self._proxy_url,
        )

        self._client = await WebSocketClient.connect(
            loop_=self._loop,
            config=config,
            handler=self._handle_message,
            post_reconnection=self._handle_reconnect,
        )
        self._log.info(f"Connected to {self._base_url}", LogColor.BLUE)

    def _cancel_keepalive(self) -> None:
        if self._keepalive_task is not None:
            self._keepalive_task.cancel()
            self._keepalive_task = None

    def _handle_reconnect(self) -> None:
        self._is_authenticated = False
        self._subscription_id = None
        self._cancel_keepalive()
        self._reconnect_task = self._loop.create_task(self._reauth_and_resubscribe())

    async def _reauth_and_resubscribe(self) -> None:
        async with self._recovery_lock:
            try:
                self._log.warning("Reconnected, re-authenticating...")
                await self._disconnect_stream()
                await self.session_logon()
                await self.subscribe_user_data_stream(
                    pre_dispatch_hook=self._on_resubscribe,
                )
                self._log.warning("Re-authenticated and resubscribed after reconnect")
            except Exception as e:
                self._is_recovery_failed = True
                self._log.error(
                    f"Failed to re-authenticate after reconnect: {e}, "
                    "user data stream is NOT active, disconnecting",
                )
                await self.disconnect()

    def _ws_api_reconnecting(self) -> bool:
        if self._use_rest_listen_key or self._client is None:
            return False
        # A queued _reauth_and_resubscribe counts as an in-flight reconnect
        # even after the socket finished its TCP handshake. Otherwise a
        # concurrent _resubscribe can slip through between TCP reconnect and
        # session.logon and rotate the stream a second time.
        if self._reconnect_task is not None and not self._reconnect_task.done():
            return True
        return self._client.is_reconnecting() or self._client.is_disconnecting()

    async def _resubscribe(self) -> None:
        # Fast-path: if the WS API socket is already reconnecting, defer. Its
        # own post_reconnection handler will run _reauth_and_resubscribe once
        # it comes back and rotate the listenKey and stream together.
        if self._ws_api_reconnecting():
            self._log.debug("Skipping resubscribe, WS API client is reconnecting")
            return

        async with self._recovery_lock:
            await self._resubscribe_locked()

    async def _resubscribe_locked(self) -> bool:
        # Re-check under the lock: the WS API could have entered reconnect
        # between the fast-path check and here.
        if self._ws_api_reconnecting():
            self._log.debug("Deferring resubscribe under lock, WS API is reconnecting")
            return False

        self._subscription_id = None
        self._cancel_keepalive()
        await self._disconnect_stream()

        if self._use_rest_listen_key:
            try:
                await self._subscribe_rest(pre_dispatch_hook=self._on_resubscribe)
                self._log.warning("Resubscribed after listenKey expiry (REST)")
                return True
            except Exception as e:
                self._is_recovery_failed = True
                self._log.error(
                    f"Failed to recover REST listenKey: {e}, "
                    "user data stream is NOT active, disconnecting",
                )
                await self.disconnect()
                return False

        try:
            await self.subscribe_user_data_stream(pre_dispatch_hook=self._on_resubscribe)
            self._log.warning("Resubscribed after stream termination")
            return True
        except Exception as e:
            # Session may have silently expired, fall back to full re-auth
            self._log.warning(f"Resubscribe failed ({e}), re-authenticating...")
            try:
                await self.session_logon()
                await self.subscribe_user_data_stream(pre_dispatch_hook=self._on_resubscribe)
                self._log.warning("Re-authenticated and resubscribed after stream termination")
                return True
            except Exception as e:
                # If the failure was because the WS API dropped into reconnect
                # between our check and the send, let its own reconnect handler
                # recover; do not trip the permanent _is_recovery_failed flag.
                if self._ws_api_reconnecting():
                    self._log.warning(
                        f"Resubscribe raced WS API reconnect ({e}), "
                        "deferring to _reauth_and_resubscribe",
                    )
                    return False
                self._is_recovery_failed = True
                self._log.error(
                    f"Failed to recover after stream termination: {e}, "
                    "user data stream is NOT active, disconnecting",
                )
                await self.disconnect()
                return False

    async def _disconnect_stream(self) -> None:
        if self._stream_client is not None:
            try:
                await self._stream_client.disconnect()
            except WebSocketClientError as e:
                self._log.error(f"Error disconnecting stream: {e}")
            self._stream_client = None

    async def disconnect(self) -> None:
        """
        Disconnect from the WebSocket API server and stream connection.
        """
        self._cancel_keepalive()
        self._subscription_id = None
        self._is_authenticated = False

        await self._disconnect_stream()

        if self._use_rest_listen_key:
            return

        if self._client is None:
            return

        if self._client.is_disconnecting() or self._client.is_closed():
            self._log.debug("Already disconnecting/closed, skipping")
            return

        self._log.debug("Disconnecting...")
        try:
            await self._client.disconnect()
        except WebSocketClientError as e:
            self._log.error(str(e))

        self._client = None
        self._log.info(f"Disconnected from {self._base_url}", LogColor.BLUE)

    async def _send_request(
        self,
        method: str,
        params: dict[str, Any] | None = None,
        timeout: float = 10.0,
    ) -> dict[str, Any]:
        if self._is_recovery_failed:
            raise RuntimeError(
                "User data stream recovery failed, cannot send requests",
            )
        if self._client is None:
            raise RuntimeError("WebSocket client not connected")

        msg_id = self._next_msg_id()
        request = {
            "id": msg_id,
            "method": method,
            "params": params or {},
        }

        future: asyncio.Future[dict[str, Any]] = self._loop.create_future()
        self._pending_requests[msg_id] = future

        self._log.debug(f"SENDING: {request}")
        try:
            await self._client.send_text(msgspec.json.encode(request))
            response = await asyncio.wait_for(future, timeout=timeout)
        except TimeoutError:
            self._pending_requests.pop(msg_id, None)
            raise TimeoutError(f"Request {method} timed out after {timeout}s")
        except WebSocketClientError as e:
            self._pending_requests.pop(msg_id, None)
            raise RuntimeError(f"Failed to send request: {e}")

        self._log.debug(f"RECEIVED: {response}")

        if "error" in response:
            error = response["error"]
            raise RuntimeError(f"Request {method} failed: {error.get('msg', error)}")

        return response

    async def session_logon(self) -> dict[str, Any]:
        """
        Authenticate the WebSocket session using session.logon.

        Returns the session logon response.

        """
        if self._use_rest_listen_key:
            # REST listenKey auth is per-request via API key header
            self._is_authenticated = True
            self._log.info("Session authenticated (REST listenKey mode)", LogColor.GREEN)
            return {}

        timestamp = self._clock.timestamp_ms()
        sign_params = f"apiKey={self._api_key}&timestamp={timestamp}"
        signature = self._get_sign(sign_params)

        params = {
            "apiKey": self._api_key,
            "timestamp": timestamp,
            "signature": signature,
        }

        response = await self._send_request("session.logon", params)
        self._is_authenticated = True
        self._log.info("Session authenticated", LogColor.GREEN)
        return response

    async def session_status(self) -> dict[str, Any]:
        """
        Query the current session status.
        """
        return await self._send_request("session.status")

    async def session_logout(self) -> dict[str, Any]:
        """
        Logout from the WebSocket session.
        """
        response = await self._send_request("session.logout")
        self._is_authenticated = False
        self._log.info("Session logged out")
        return response

    async def subscribe_user_data_stream(
        self,
        pre_dispatch_hook: Callable[[], Awaitable[None]] | None = None,
    ) -> str:
        """
        Subscribe to the user data stream.

        For Spot, sends `userDataStream.subscribe` — events arrive inline.
        For Futures + Ed25519, sends `userDataStream.start` via WS API.
        For Futures + HMAC, creates listenKey via REST API.

        The optional `pre_dispatch_hook` is awaited after the stream connects
        but before buffered events are released to the handler. Resubscribe
        callers pass reconciliation here so a stale REST snapshot cannot
        replay closed-order state over fresh stream events: the WebSocket is
        already connected (so Binance is pushing events to us) and any
        message received during the hook is queued and then drained in order.

        """
        if not self._is_authenticated:
            raise RuntimeError("Session not authenticated, call session_logon first")

        if self._use_rest_listen_key:
            return await self._subscribe_rest(pre_dispatch_hook)

        if self._is_futures:
            response = await self._send_request("userDataStream.start")
            result = response.get("result", {})
            listen_key = result.get("listenKey")
            if listen_key is None:
                raise RuntimeError(f"No listenKey in response: {response}")
            self._subscription_id = listen_key
            self._log.info(f"Started user data stream: {mask_api_key(listen_key)}", LogColor.GREEN)

            await self._connect_stream_with_hook(listen_key, pre_dispatch_hook)

            self._keepalive_task = self._loop.create_task(self._keepalive_loop())
            return listen_key
        else:
            # Spot uses a single WS connection; the subscribe request and event
            # delivery share that connection. Pause dispatch around the request
            # + hook so queued event deliveries are drained in order.
            if pre_dispatch_hook is not None:
                self._dispatch_paused = True
            try:
                response = await self._send_request("userDataStream.subscribe")
                result = response.get("result", {})
                subscription_id = result.get("subscriptionId")
                if subscription_id is None:
                    raise RuntimeError(f"No subscriptionId in response: {response}")
                self._subscription_id = str(subscription_id)
                self._log.info(
                    f"Subscribed to user data stream: {subscription_id}",
                    LogColor.BLUE,
                )

                if pre_dispatch_hook is not None:
                    await self._safe_pre_dispatch_hook(pre_dispatch_hook)
            finally:
                if pre_dispatch_hook is not None:
                    self._resume_dispatch()

            return str(subscription_id)

    async def _subscribe_rest(
        self,
        pre_dispatch_hook: Callable[[], Awaitable[None]] | None = None,
    ) -> str:
        response = await self._http_user.create_listen_key()
        listen_key = response.listenKey
        self._subscription_id = listen_key
        self._log.info(
            f"Created listenKey (REST): {mask_api_key(listen_key)}",
            LogColor.GREEN,
        )

        await self._connect_stream_with_hook(listen_key, pre_dispatch_hook)

        self._keepalive_task = self._loop.create_task(self._keepalive_loop())
        return listen_key

    async def _connect_stream_with_hook(
        self,
        listen_key: str,
        pre_dispatch_hook: Callable[[], Awaitable[None]] | None,
    ) -> None:
        if pre_dispatch_hook is None:
            await self._connect_stream(listen_key)
            return

        # Pause dispatch before connect so any events arriving on the new
        # stream are buffered, not dispatched, while the hook runs.
        self._dispatch_paused = True
        try:
            await self._connect_stream(listen_key)
            await self._safe_pre_dispatch_hook(pre_dispatch_hook)
        finally:
            self._resume_dispatch()

    async def _safe_pre_dispatch_hook(
        self,
        hook: Callable[[], Awaitable[None]],
    ) -> None:
        try:
            await hook()
        except Exception as e:
            self._log.error(f"pre_dispatch_hook failed: {e}")

    def _resume_dispatch(self) -> None:
        self._dispatch_paused = False
        if not self._dispatch_buffer:
            return

        buffered = self._dispatch_buffer
        self._dispatch_buffer = []
        self._log.debug(f"Draining {len(buffered)} buffered stream event(s)")
        for raw in buffered:
            self._handler(raw)

    async def _connect_stream(self, listen_key: str) -> None:
        if self._stream_base_url is None:
            raise RuntimeError("stream_base_url is required for futures")

        stream_url = f"{self._stream_base_url}/ws?listenKey={listen_key}"
        self._log.debug(
            f"Connecting stream to {self._stream_base_url}/ws?listenKey={mask_api_key(listen_key)}...",
        )

        config = WebSocketConfig(
            url=stream_url,
            headers=[],
            heartbeat=60,
            proxy_url=self._proxy_url,
        )

        # The stream connection can drop independently of the WS API client
        # (e.g. listenKey deleted by another process, heartbeat timeout). Always
        # rotate the listenKey on reconnect so we never reuse a stale one.
        self._stream_client = await WebSocketClient.connect(
            loop_=self._loop,
            config=config,
            handler=self._handle_stream_message,
            post_reconnection=self._handle_stream_reconnect,
        )
        self._log.info(
            f"Connected stream to {self._stream_base_url}/ws?listenKey={mask_api_key(listen_key)}",
            LogColor.BLUE,
        )

    def _handle_stream_reconnect(self) -> None:
        self._log.warning("Stream reconnected, creating new listenKey...")
        self._loop.create_task(self._resubscribe())

    async def unsubscribe_user_data_stream(self) -> None:
        """
        Unsubscribe from the user data stream.
        """
        self._cancel_keepalive()

        if self._subscription_id is None:
            self._log.warning("No active subscription to unsubscribe from")
            return

        if self._use_rest_listen_key:
            await self._disconnect_stream()
            try:
                await self._http_user.close_listen_key()
            except Exception as e:
                self._log.warning(f"Error closing REST listenKey: {e}")
            self._log.info("Closed user data stream (REST)")
        elif self._is_futures:
            await self._disconnect_stream()
            await self._send_request(
                "userDataStream.stop",
                {"listenKey": self._subscription_id},
            )
            self._log.info("Stopped user data stream")
        else:
            await self._send_request(
                "userDataStream.unsubscribe",
                {"subscriptionId": self._subscription_id},
            )
            self._log.info(
                f"Unsubscribed from user data stream: {self._subscription_id}",
            )
        self._subscription_id = None

    async def _keepalive_loop(self) -> None:
        consecutive_failures = 0

        try:
            while True:
                await asyncio.sleep(self._KEEPALIVE_INTERVAL_SECS)
                try:
                    if self._use_rest_listen_key:
                        await self._http_user.keepalive_listen_key()
                    else:
                        await self._send_request(
                            "userDataStream.ping",
                            {"listenKey": self._subscription_id},
                        )
                    self._log.debug("User data stream keepalive sent")
                    consecutive_failures = 0
                except Exception as e:
                    consecutive_failures += 1
                    self._log.error(
                        f"Failed to send keepalive ({consecutive_failures}/"
                        f"{self._MAX_KEEPALIVE_FAILURES}): {e}",
                    )

                    if consecutive_failures >= self._MAX_KEEPALIVE_FAILURES:
                        self._log.warning(
                            "Keepalive failed "
                            f"{consecutive_failures} consecutive times, resubscribing",
                        )
                        self._loop.create_task(self._resubscribe())
                        return
        except asyncio.CancelledError:
            pass
