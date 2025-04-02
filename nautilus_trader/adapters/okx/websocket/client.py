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
import base64
from collections import defaultdict
from collections.abc import Awaitable
from collections.abc import Callable
from typing import Any, Literal, ParamSpec, TypeVar

import msgspec
import pandas as pd

from nautilus_trader.adapters.okx.common.credentials import get_api_key
from nautilus_trader.adapters.okx.common.credentials import get_api_secret
from nautilus_trader.adapters.okx.common.credentials import get_passphrase
from nautilus_trader.adapters.okx.common.enums import OKXBarSize
from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType
from nautilus_trader.adapters.okx.common.enums import OKXOrderSide
from nautilus_trader.adapters.okx.common.enums import OKXOrderType
from nautilus_trader.adapters.okx.common.enums import OKXPositionSide
from nautilus_trader.adapters.okx.common.enums import OKXTradeMode
from nautilus_trader.adapters.okx.common.enums import OKXWsBaseUrlType
from nautilus_trader.adapters.okx.common.urls import get_ws_base_url
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.nautilus_pyo3 import WebSocketClient
from nautilus_trader.core.nautilus_pyo3 import WebSocketClientError
from nautilus_trader.core.nautilus_pyo3 import WebSocketConfig
from nautilus_trader.core.nautilus_pyo3 import hmac_signature


MAX_ARGS_PER_SUBSCRIPTION_REQUEST = 10  # not OKX limit but smart; carried over from Bybit adapter

SUBSCRIBE_UNSUBSCRIBE_LOGIN_LIMIT_PER_HOUR = 480


T = TypeVar("T")
P = ParamSpec("P")


def check_public(asyncmethod: Callable[P, Awaitable[T]]) -> Callable[P, Awaitable[T]]:
    assert asyncio.iscoroutinefunction(asyncmethod), f"{asyncmethod} is expected to be a coroutine"

    async def wrapper(*args: P.args, **kwargs: P.kwargs) -> T:
        self: OKXWebsocketClient = args[0]  # type: ignore
        assert (
            self.ws_base_url_type == OKXWsBaseUrlType.PUBLIC
        ), f"`{asyncmethod.__name__}` requires `ws_base_url_type` of {OKXWsBaseUrlType.PUBLIC}"
        return await asyncmethod(*args, **kwargs)

    return wrapper


def check_private(asyncmethod: Callable[P, Awaitable[T]]) -> Callable[P, Awaitable[T]]:
    assert asyncio.iscoroutinefunction(asyncmethod), f"{asyncmethod} is expected to be a coroutine"

    async def wrapper(*args: P.args, **kwargs: P.kwargs) -> T:
        self: OKXWebsocketClient = args[0]  # type: ignore
        assert (
            self.ws_base_url_type == OKXWsBaseUrlType.PRIVATE
        ), f"`{asyncmethod.__name__}` requires `ws_base_url_type` of {OKXWsBaseUrlType.PRIVATE}"
        return await asyncmethod(*args, **kwargs)

    return wrapper


def check_business(asyncmethod: Callable[P, Awaitable[T]]) -> Callable[P, Awaitable[T]]:
    assert asyncio.iscoroutinefunction(asyncmethod), f"{asyncmethod} is expected to be a coroutine"

    async def wrapper(*args: P.args, **kwargs: P.kwargs) -> T:
        self: OKXWebsocketClient = args[0]  # type: ignore
        assert (
            self.ws_base_url_type == OKXWsBaseUrlType.BUSINESS
        ), f"`{asyncmethod.__name__}` requires `ws_base_url_type` of {OKXWsBaseUrlType.BUSINESS}"
        return await asyncmethod(*args, **kwargs)

    return wrapper


OKX_CHANNEL_WS_BASE_URL_TYPE_MAP = {
    # Public:
    "tickers": OKXWsBaseUrlType.PUBLIC,
    "trades": OKXWsBaseUrlType.PUBLIC,
    "bbo-tbt": OKXWsBaseUrlType.PUBLIC,
    "books50-l2-tbt": OKXWsBaseUrlType.PUBLIC,
    # Private:
    "account": OKXWsBaseUrlType.PRIVATE,
    "positions": OKXWsBaseUrlType.PRIVATE,
    "balance_and_position": OKXWsBaseUrlType.PRIVATE,
    "liquidation-warning": OKXWsBaseUrlType.PRIVATE,
    "account-greeks": OKXWsBaseUrlType.PRIVATE,
    "orders": OKXWsBaseUrlType.PRIVATE,
    "fills": OKXWsBaseUrlType.PRIVATE,
    # Business:
    "trades-all": OKXWsBaseUrlType.BUSINESS,
    **{f"candle{bar_size.value}": OKXWsBaseUrlType.BUSINESS for bar_size in list(OKXBarSize)},
}

SUPPORTED_OKX_ORDER_BOOK_DEPTH_CHANNELS = {
    1: "bbo-tbt",
    50: "books50-l2-tbt",
    400: "books-l2-tbt",
    # NOTE: exclude "books" channel b/c it is also depth 400 but updates slower
    # NOTE: exclude "books5" channel b/c it only provides snapshots
}

SUPPORTED_WS_DEPTHS = Literal[1, 50, 400]


def get_book_channel(depth: SUPPORTED_WS_DEPTHS) -> str:
    assert depth in SUPPORTED_OKX_ORDER_BOOK_DEPTH_CHANNELS, (
        f"OKX does not support orderbook subscriptions of depth {depth}. Supported depths (and "
        f"their channels are {SUPPORTED_OKX_ORDER_BOOK_DEPTH_CHANNELS})"
    )
    return SUPPORTED_OKX_ORDER_BOOK_DEPTH_CHANNELS[depth]


def get_ws_url(base_url_ws: str, ws_base_url_type: OKXWsBaseUrlType) -> str:
    match ws_base_url_type:
        case OKXWsBaseUrlType.PUBLIC:
            return f"{base_url_ws}/public"
        case OKXWsBaseUrlType.PRIVATE:
            return f"{base_url_ws}/private"
        case OKXWsBaseUrlType.BUSINESS:
            return f"{base_url_ws}/business"
        case _:
            # Theoretically unreachable but retained to keep the match exhaustive
            raise ValueError(
                f"unknown websocket base url type {ws_base_url_type} - must be one of "
                f"{list(OKXWsBaseUrlType)}",
            )


class OKXWebsocketClient:
    """
    Provides an OKX streaming WebSocket client.

    Parameters
    ----------
    clock : LiveClock
        The clock instance.
    handler : Callable[[bytes], None]
        The callback handler for message events.
    handler_reconnect : Callable[..., Awaitable[None]], optional
        The callback handler to be called on reconnect.
    api_key : str, optional
        The OKX API public key.
    api_secret : str, optional
        The OKX API secret key.
    passphrase : str, optional
        The passphrase used when creating the OKX API keys.
    base_url: str, None
        The base websocket endpoint URL for the client. If None, the url will be chosen from
        `get_ws_base_url(is_demo=...)`
    ws_base_url_type: OKXWsBaseUrlType
        The websocket's base url type, one of
        {OKXWsBaseUrlType.PUBLIC, OKXWsBaseUrlType.PRIVATE, OKXWsBaseUrlType.BUSINESS}.
    is_demo : bool
        Whether the client is to be used for demo trading or not.
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    login_for_tbt_orderbooks : bool, [default=False]
        Whether this websocket client is to be used for tick-by-tick order book subscriptions which
        requires clients to be logged-in by OKX.

    """

    def __init__(
        self,
        clock: LiveClock,
        handler: Callable[[bytes], None] | None,
        handler_reconnect: Callable[..., Awaitable[None]] | None,
        api_key: str | None,
        api_secret: str | None,
        passphrase: str | None,
        base_url: str | None,
        ws_base_url_type: OKXWsBaseUrlType,
        is_demo: bool,
        loop: asyncio.AbstractEventLoop,
        login_for_tbt_orderbooks: bool = False,
    ) -> None:
        self._is_private = (
            ws_base_url_type in [OKXWsBaseUrlType.PRIVATE, OKXWsBaseUrlType.BUSINESS]
            or login_for_tbt_orderbooks
        )
        if self._is_private:
            api_key = get_api_key(is_demo) if api_key is None else api_key
            api_secret = get_api_secret(is_demo) if api_secret is None else api_secret
            passphrase = get_passphrase(is_demo) if passphrase is None else passphrase
            assert (
                api_key and api_secret and passphrase
            ), "private websocket client needs api_key, api_secret, and passphrase for logging in"

        self._clock = clock
        self._log: Logger = Logger(name=type(self).__name__)

        self._base_url: str = get_ws_url(base_url or get_ws_base_url(is_demo), ws_base_url_type)
        self._handler: Callable[[bytes], None] = handler or self.default_handler
        self._handler_reconnect: Callable[..., Awaitable[None]] | None = handler_reconnect
        self._loop = loop

        self._client: WebSocketClient | None = None
        self._api_key = api_key
        self._api_secret = api_secret
        self._passphrase = passphrase
        self._ws_base_url_type = ws_base_url_type
        self._is_running = False

        self._subscriptions: list[dict[str, Any]] = []
        self._channel_counts: dict[str, int] = defaultdict(int)
        self._last_pong = pd.Timestamp(0, tz="UTC")
        self._pong_max_age = pd.Timedelta(minutes=1)  # should receive a pong every 20 seconds
        self._check_pong_age_task: asyncio.Task | None = None

    @property
    def subscriptions(self) -> list[dict[str, Any]]:
        return self._subscriptions

    @property
    def ws_base_url_type(self) -> OKXWsBaseUrlType:
        return self._ws_base_url_type

    @property
    def is_private(self) -> bool:
        return self._is_private

    @property
    def channel_counts(self) -> dict[str, int]:
        return self._channel_counts.copy()

    @property
    def is_connected(self) -> bool:
        if self._client is None:
            return False
        return self._client.is_active() if hasattr(self._client, "is_alive") else False

    def set_handler(self, handler: Callable[[bytes], None]) -> None:
        self._handler = handler

    def update_channel_count(self, channel: str, count: int) -> None:
        self._channel_counts[channel] = count

    def get_subscription_count(self, channel: str | None = None) -> int:
        if channel:
            return self._channel_counts[channel]
        return len(self.subscriptions)

    def has_subscription(self, item: dict[str, Any]) -> bool:
        return item in self._subscriptions

    def default_handler(self, raw: bytes) -> None:
        response = msgspec.json.decode(raw)
        self._log.debug(f"Received websocket response: {response}")

    async def connect(self) -> None:
        if self._check_pong_age_task:
            try:
                self._check_pong_age_task.cancel()
            except Exception:  # noqa: S110
                pass
        self._check_pong_age_task = None

        if self._client:
            try:
                await self._client.disconnect()
            except WebSocketClientError as e:
                self._log.error(str(e))

            self._client = None  # Dispose (will go out of scope)
            self._is_running = False

        self._log.debug(f"Connecting to {self._base_url} websocket stream")
        config = WebSocketConfig(
            url=self._base_url,
            handler=self._handler,
            heartbeat=20,
            heartbeat_msg="ping",
            headers=[],
            ping_handler=self._handle_ping,
        )
        client = await WebSocketClient.connect(
            config=config,
            post_connection=None,
            post_reconnection=self.reconnect,
            post_disconnection=None,
        )
        self._client = client
        self._is_running = True
        self._log.info(f"Connected to {self._base_url}", LogColor.BLUE)

        ## Authenticate
        if self._is_private:
            self._log.info(
                "Attempting to login private websocket client. Please check ws messages to "
                "determine if login was successful...",
            )
            await self._login()

        self._last_pong = self._clock.utc_now()
        self._check_pong_age_task = self._loop.create_task(self._reconnect_on_pong_max_age())

    def _handle_ping(self, raw: bytes) -> None:
        self._log.debug(f"Handling ping, sending pong msg: {raw.decode()}")
        self._loop.create_task(self.send_pong(raw))

    async def send_pong(self, raw: bytes) -> None:
        """
        Send the given raw payload to the server as a PONG message.
        """
        if self._client is None:
            return

        try:
            await self._client.send_text(raw)
        except WebSocketClientError as e:
            self._log.error(str(e))

    def reconnect(self) -> None:
        """
        Reconnect the client to the server and resubscribe to all streams.
        """
        if not self._is_running:
            return

        self._log.warning(f"Trying to reconnect to {self._base_url}")
        self._loop.create_task(self._reconnect_wrapper())  # type: ignore

    async def _reconnect_wrapper(self):
        if self._check_pong_age_task:
            try:
                self._check_pong_age_task.cancel()
            except Exception:  # noqa: S110
                pass
        self._check_pong_age_task = None

        ## Authenticate
        if self._is_private:
            self._log.info(
                "Attempting to login private websocket client. Please check ws messages to "
                "determine if login was successful...",
            )
            await self._login()

        # Re-subscribe to all streams
        await self._subscribe_all()

        # Run reconnect handler
        if self._handler_reconnect:
            await self._handler_reconnect()

        self._last_pong = self._clock.utc_now()
        self._check_pong_age_task = self._loop.create_task(self._reconnect_on_pong_max_age())

        self._log.warning(f"Reconnected to {self._base_url}")

    async def disconnect(self) -> None:
        self._is_running = False

        if self._client is None:
            self._log.warning("Cannot disconnect: not connected")
            return

        await self._client.disconnect()
        self._client = None  # Dispose (will go out of scope)

        self._log.info(f"Disconnected from {self._base_url}", LogColor.BLUE)

    async def _reconnect_on_pong_max_age(self) -> None:
        def _connect_later():
            self._loop.create_task(self.connect())

        while True:
            await asyncio.sleep(self._pong_max_age.total_seconds() + 10)
            age = self._clock.utc_now() - self._last_pong
            if age > self._pong_max_age:
                self._log.warning(
                    f"Last pong age {age} exceeds max age {self._pong_max_age}. Reconnecting...",
                )
                self._loop.call_later(1.0, _connect_later)
                return

    ################################################################################
    # Public
    ################################################################################

    @check_public
    async def subscribe_tickers(self, instId: str) -> None:
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected")
            return

        subscription = {"channel": "tickers", "instId": instId}

        if subscription in self._subscriptions:
            return

        self._subscriptions.append(subscription)
        payload = {"op": "subscribe", "args": [subscription]}
        await self._send(payload)

    @check_public
    async def unsubscribe_tickers(self, instId: str) -> None:
        if self._client is None:
            self._log.warning("Cannot unsubscribe: not connected")
            return

        subscription = {"channel": "tickers", "instId": instId}

        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        payload = {"op": "unsubscribe", "args": [subscription]}
        await self._send(payload)

    @check_public
    async def subscribe_trades(self, instId: str) -> None:
        """
        Subscribe to aggregated trades (one update per taker order).
        """
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected")
            return

        subscription = {"channel": "trades", "instId": instId}

        if subscription in self._subscriptions:
            return

        self._subscriptions.append(subscription)
        payload = {"op": "subscribe", "args": [subscription]}
        await self._send(payload)

    @check_public
    async def unsubscribe_trades(self, instId: str) -> None:
        """
        Unsubscribe from aggregated trades (one update per taker order).
        """
        if self._client is None:
            self._log.warning("Cannot unsubscribe: not connected")
            return

        subscription = {"channel": "trades", "instId": instId}

        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        payload = {"op": "unsubscribe", "args": [subscription]}
        await self._send(payload)

    @check_public
    async def subscribe_order_book(self, instId: str, depth: SUPPORTED_WS_DEPTHS) -> None:
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected")
            return

        subscription = {"channel": get_book_channel(depth), "instId": instId}

        if subscription in self._subscriptions:
            return

        self._subscriptions.append(subscription)
        payload = {"op": "subscribe", "args": [subscription]}
        await self._send(payload)

    @check_public
    async def unsubscribe_order_book(self, instId: str, depth: SUPPORTED_WS_DEPTHS) -> None:
        if self._client is None:
            self._log.warning("Cannot unsubscribe: not connected")
            return

        subscription = {"channel": get_book_channel(depth), "instId": instId}

        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        payload = {"op": "unsubscribe", "args": [subscription]}
        await self._send(payload)

    ################################################################################
    # Business
    ################################################################################

    @check_business
    async def subscribe_candlesticks(self, instId: str, bar_size: OKXBarSize) -> None:
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected")
            return

        subscription = {"channel": f"candle{bar_size.value}", "instId": instId}

        if subscription in self._subscriptions:
            return

        self._subscriptions.append(subscription)
        payload = {"op": "subscribe", "args": [subscription]}
        await self._send(payload)

    @check_business
    async def unsubscribe_candlesticks(self, instId: str, bar_size: OKXBarSize) -> None:
        if self._client is None:
            self._log.warning("Cannot unsubscribe: not connected")
            return

        subscription = {"channel": f"candle{bar_size.value}", "instId": instId}

        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        payload = {"op": "unsubscribe", "args": [subscription]}
        await self._send(payload)

    @check_business
    async def subscribe_trades_all(self, instId: str) -> None:
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected")
            return

        subscription = {"channel": "trades-all", "instId": instId}

        if subscription in self._subscriptions:
            return

        self._subscriptions.append(subscription)
        payload = {"op": "subscribe", "args": [subscription]}
        await self._send(payload)

    @check_business
    async def unsubscribe_trades_all(self, instId: str) -> None:
        if self._client is None:
            self._log.warning("Cannot unsubscribe: not connected")
            return

        subscription = {"channel": "trades-all", "instId": instId}

        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        payload = {"op": "unsubscribe", "args": [subscription]}
        await self._send(payload)

    ################################################################################
    # Private
    ################################################################################

    @check_private
    async def subscribe_account(self) -> None:
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected")
            return

        subscription = {"channel": "account"}

        if subscription in self._subscriptions:
            return

        self._subscriptions.append(subscription)
        payload = {"op": "subscribe", "args": [subscription]}
        await self._send(payload)

    @check_private
    async def unsubscribe_account(self) -> None:
        if self._client is None:
            self._log.warning("Cannot unsubscribe: not connected")
            return

        subscription = {"channel": "account"}

        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        payload = {"op": "unsubscribe", "args": [subscription]}
        await self._send(payload)

    @check_private
    async def subscribe_positions(
        self,
        instType: OKXInstrumentType = OKXInstrumentType.ANY,
        instFamily: str | None = None,
        instId: str | None = None,
    ) -> None:
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected")
            return

        subscription = {"channel": "positions", "instType": instType.value}

        if instFamily:
            subscription.update({"instFamily": instFamily})
        if instId:
            subscription.update({"instId": instId})

        if subscription in self._subscriptions:
            return

        self._subscriptions.append(subscription)
        payload = {"op": "subscribe", "args": [subscription]}
        await self._send(payload)

    @check_private
    async def unsubscribe_positions(
        self,
        instType: OKXInstrumentType = OKXInstrumentType.ANY,
        instFamily: str | None = None,
        instId: str | None = None,
    ) -> None:
        if self._client is None:
            self._log.warning("Cannot unsubscribe: not connected")
            return

        subscription = {"channel": "positions", "instType": instType.value}

        if instFamily:
            subscription.update({"instFamily": instFamily})
        if instId:
            subscription.update({"instId": instId})

        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        payload = {"op": "unsubscribe", "args": [subscription]}
        await self._send(payload)

    @check_private
    async def subscribe_balance_and_position(self) -> None:
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected")
            return

        subscription = {"channel": "balance_and_position"}

        if subscription in self._subscriptions:
            return

        self._subscriptions.append(subscription)
        payload = {"op": "subscribe", "args": [subscription]}
        await self._send(payload)

    @check_private
    async def unsubscribe_balance_and_position(self) -> None:
        if self._client is None:
            self._log.warning("Cannot unsubscribe: not connected")
            return

        subscription = {"channel": "balance_and_position"}

        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        payload = {"op": "unsubscribe", "args": [subscription]}
        await self._send(payload)

    @check_private
    async def subscribe_liquidation_warning(
        self,
        instType: OKXInstrumentType = OKXInstrumentType.ANY,
        instFamily: str | None = None,
        instId: str | None = None,
    ) -> None:
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected")
            return

        subscription = {"channel": "liquidation-warning", "instType": instType.value}
        if instFamily:
            subscription.update({"instFamily": instFamily})
        if instId:
            subscription.update({"instId": instId})

        if subscription in self._subscriptions:
            return

        self._subscriptions.append(subscription)
        payload = {"op": "subscribe", "args": [subscription]}
        await self._send(payload)

    @check_private
    async def unsubscribe_liquidation_warning(
        self,
        instType: OKXInstrumentType = OKXInstrumentType.ANY,
        instFamily: str | None = None,
        instId: str | None = None,
    ) -> None:
        if self._client is None:
            self._log.warning("Cannot unsubscribe: not connected")
            return

        subscription = {"channel": "liquidation-warning", "instType": instType.value}
        if instFamily:
            subscription.update({"instFamily": instFamily})
        if instId:
            subscription.update({"instId": instId})

        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        payload = {"op": "unsubscribe", "args": [subscription]}
        await self._send(payload)

    @check_private
    async def subscribe_account_greeks(self) -> None:
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected")
            return

        subscription = {"channel": "account-greeks"}

        if subscription in self._subscriptions:
            return

        self._subscriptions.append(subscription)
        payload = {"op": "subscribe", "args": [subscription]}
        await self._send(payload)

    @check_private
    async def unsubscribe_account_greeks(self) -> None:
        if self._client is None:
            self._log.warning("Cannot unsubscribe: not connected")
            return

        subscription = {"channel": "account-greeks"}

        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        payload = {"op": "unsubscribe", "args": [subscription]}
        await self._send(payload)

    @check_private
    async def subscribe_orders(
        self,
        instType: OKXInstrumentType = OKXInstrumentType.ANY,
        instFamily: str | None = None,
        instId: str | None = None,
    ) -> None:
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected")
            return

        subscription = {"channel": "orders", "instType": instType.value}
        if instFamily:
            subscription.update({"instFamily": instFamily})
        if instId:
            subscription.update({"instId": instId})

        if subscription in self._subscriptions:
            return

        self._subscriptions.append(subscription)
        payload = {"op": "subscribe", "args": [subscription]}
        await self._send(payload)

    @check_private
    async def unsubscribe_orders(
        self,
        instType: OKXInstrumentType = OKXInstrumentType.ANY,
        instFamily: str | None = None,
        instId: str | None = None,
    ) -> None:
        if self._client is None:
            self._log.warning("Cannot unsubscribe: not connected")
            return

        subscription = {"channel": "orders", "instType": instType.value}
        if instFamily:
            subscription.update({"instFamily": instFamily})
        if instId:
            subscription.update({"instId": instId})

        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        payload = {"op": "unsubscribe", "args": [subscription]}
        await self._send(payload)

    @check_private
    async def subscribe_fills(self) -> None:
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected")
            return

        subscription = {"channel": "fills"}

        if subscription in self._subscriptions:
            return

        self._subscriptions.append(subscription)
        payload = {"op": "subscribe", "args": [subscription]}
        await self._send(payload)

    @check_private
    async def unsubscribe_fills(self) -> None:
        if self._client is None:
            self._log.warning("Cannot unsubscribe: not connected")
            return

        subscription = {"channel": "fills"}

        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        payload = {"op": "unsubscribe", "args": [subscription]}
        await self._send(payload)

    ################################################################################
    # Private execution ops
    ################################################################################

    @check_private
    async def place_order(
        self,
        msg_id: str,  # up to 32 characters, value is returned in response in 'id' field
        instId: str,
        tdMode: OKXTradeMode,
        side: OKXOrderSide,
        ordType: OKXOrderType,
        sz: str,
        ccy: str | None = None,  # only applicable to cross MARGIN orders for SPOT/FUTURES
        px: str | None = None,
        reduceOnly: bool = False,
        posSide: OKXPositionSide = OKXPositionSide.NET,  # only applicable to FUTURES/SWAP
        expTime: str | None = None,  # unit time in milliseconds
        clOrdId: str | None = None,  # up to 32 characters
        tag: str | None = None,  # up to 16 characters
    ) -> None:
        # TODO: how to enforce rate limits?
        # -> https://www.okx.com/docs-v5/en/#order-book-trading-trade-ws-place-order

        if self._client is None:
            self._log.warning("Cannot place order: not connected")
            return

        if px is None:
            assert ordType in [
                OKXOrderType.MARKET,
                OKXOrderType.OPTIMAL_LIMIT_IOC,
            ], (
                "`px` (order price) can only be empty when `ordType` is 'market' or "
                "'optimal_limit_ioc'"
            )

        args = {
            "instId": instId,
            "tdMode": tdMode.value,
            "side": side.value,
            "ordType": ordType.value,
            "sz": sz,
            "reduceOnly": reduceOnly,
        }
        if px:
            args.update({"px": px})
        if ccy:
            args.update({"ccy": ccy})
        if posSide:
            args.update({"posSide": posSide.value})
        if expTime:
            args.update({"expTime": expTime})
        if clOrdId:
            args.update({"clOrdId": clOrdId})
        if tag:
            args.update({"tag": tag})

        payload = {"id": msg_id, "op": "order", "args": [args]}

        self._log.debug(f"Sending order: {payload}")
        await self._send(payload)

    @check_private
    async def cancel_order(
        self,
        msg_id: str,
        instId: str,
        ordId: str | None = None,
        clOrdId: str | None = None,
    ) -> None:
        # TODO: how to enforce rate limits?
        # -> https://www.okx.com/docs-v5/en/#order-book-trading-trade-ws-place-order

        if self._client is None:
            self._log.warning("Cannot cancel order: not connected")
            return

        assert ordId or clOrdId, "either `ordId` or `clOrdId` is required"

        args = {
            "instId": instId,
        }
        if ordId:
            args.update({"ordId": ordId})
        elif clOrdId:
            args.update({"clOrdId": clOrdId})

        payload = {"id": msg_id, "op": "cancel-order", "args": [args]}

        self._log.debug(f"Sending order: {payload}")
        await self._send(payload)

    @check_private
    async def amend_order(
        self,
        msg_id: str,  # up to 32 characters, value is returned in response in 'id' field
        instId: str,
        cxlOnFail: bool = False,
        ordId: str | None = None,
        clOrdId: str | None = None,  # up to 32 characters
        reqId: str | None = None,  # up to 32 characters
        newSz: str | None = None,  # must be > 0
        newPx: str | None = None,
        expTime: str | None = None,  # unit time in milliseconds
    ) -> None:
        # TODO: how to enforce rate limits?
        # -> https://www.okx.com/docs-v5/en/#order-book-trading-trade-ws-place-order

        if self._client is None:
            self._log.warning("Cannot place order: not connected")
            return

        assert ordId or clOrdId, "either `ordId` or `clOrdId` is required"

        args = {
            "instId": instId,
            "cxlOnFail": cxlOnFail,
        }
        if ordId:
            args.update({"ordId": ordId})
        if clOrdId:
            args.update({"clOrdId": clOrdId})
        if reqId:
            args.update({"reqId": reqId})
        if newSz:
            assert float(newSz) > 0, "`newSz` must be greater than 0 when provided"
            args.update({"newSz": newSz})
        if newPx:
            args.update({"newPx": newPx})
        if expTime:
            args.update({"expTime": expTime})

        payload = {"id": msg_id, "op": "amend-order", "args": [args]}

        self._log.debug(f"Sending order: {payload}")
        await self._send(payload)

    ################################################################################
    # Helpers
    ################################################################################

    async def _subscribe_all(self) -> None:
        if self._client is None:
            self._log.error("Cannot subscribe all: not connected")
            return

        self._log.info("Resubscribing to all data streams...")

        # You can input up to 10 args for each subscription request sent to one connection
        subscription_lists = [
            self._subscriptions[i : i + MAX_ARGS_PER_SUBSCRIPTION_REQUEST]
            for i in range(0, len(self._subscriptions), MAX_ARGS_PER_SUBSCRIPTION_REQUEST)
        ]

        for subscriptions in subscription_lists:
            payload = {"op": "subscribe", "args": subscriptions}
            await self._send(payload)

    async def _login(self):
        if self._api_secret is None:
            raise ValueError("`api_secret` was `None` for private websocket")

        timestamp = int(self._clock.timestamp())
        message = str(timestamp) + "GET/users/self/verify"
        digest = hmac_signature(self._api_secret, message).encode()
        sign = base64.b64encode(digest).decode()
        payload = {
            "op": "login",
            "args": [
                {
                    "apiKey": self._api_key,
                    "passphrase": self._passphrase,
                    "timestamp": str(timestamp),
                    "sign": sign,
                },
            ],
        }
        await self._send(payload)

    async def _send(self, payload: dict[str, Any]) -> None:
        if self._client is None or not self._client.is_active():
            self._log.error(f"Cannot send msg {payload}: not connected")
            return

        self._log.debug(f"SENDING: {payload}")

        try:
            await self._client.send_text(msgspec.json.encode(payload))
        except WebSocketClientError as e:
            self._log.exception(str(e), e)
