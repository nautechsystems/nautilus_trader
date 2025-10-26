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

from __future__ import annotations

import asyncio
from typing import TYPE_CHECKING
from weakref import WeakSet

from msgspec import json as msgspec_json

import nautilus_trader
from nautilus_trader.adapters.bybit.common.enums import BybitOrderSide
from nautilus_trader.adapters.bybit.common.enums import BybitOrderType
from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.common.enums import BybitTimeInForce
from nautilus_trader.adapters.bybit.common.enums import BybitTpSlMode
from nautilus_trader.adapters.bybit.common.enums import BybitTriggerDirection
from nautilus_trader.adapters.bybit.common.enums import BybitTriggerType
from nautilus_trader.adapters.bybit.common.enums import BybitWsOrderRequestMsgOP

# fmt: off
from nautilus_trader.adapters.bybit.endpoints.trade.amend_order import BybitAmendOrderPostParams
from nautilus_trader.adapters.bybit.endpoints.trade.batch_amend_order import BybitBatchAmendOrderPostParams
from nautilus_trader.adapters.bybit.endpoints.trade.batch_cancel_order import BybitBatchCancelOrder
from nautilus_trader.adapters.bybit.endpoints.trade.batch_cancel_order import BybitBatchCancelOrderPostParams
from nautilus_trader.adapters.bybit.endpoints.trade.batch_place_order import BybitBatchPlaceOrder
from nautilus_trader.adapters.bybit.endpoints.trade.batch_place_order import BybitBatchPlaceOrderPostParams
from nautilus_trader.adapters.bybit.endpoints.trade.cancel_order import BybitCancelOrderPostParams
from nautilus_trader.adapters.bybit.endpoints.trade.place_order import BybitPlaceOrderPostParams
from nautilus_trader.adapters.bybit.http.errors import BybitError
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsAmendOrderResponseMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsBatchAmendOrderResponseMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsBatchCancelOrderResponseMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsBatchPlaceOrderResponseMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsCancelOrderResponseMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsMessageGeneral
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsOrderRequestMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsOrderResponseMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsOrderResponseMsgGeneral
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsPlaceOrderResponseMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsPrivateChannelAuthMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsTradeAuthMsg
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.secure import SecureString
from nautilus_trader.config import PositiveFloat
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import WebSocketClient
from nautilus_trader.core.nautilus_pyo3 import WebSocketClientError
from nautilus_trader.core.nautilus_pyo3 import WebSocketConfig
from nautilus_trader.core.nautilus_pyo3 import hmac_signature
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.live.cancellation import cancel_tasks_with_timeout


if TYPE_CHECKING:
    from collections.abc import Awaitable
    from collections.abc import Callable
    from typing import Any


# fmt: on

MAX_ARGS_PER_SUBSCRIPTION_REQUEST = 10

WsOrderResponseMsgFuture = asyncio.Future[BybitWsOrderResponseMsg]


class BybitWebSocketClient:
    """
    Provides a Bybit streaming WebSocket client.

    Parameters
    ----------
    clock : LiveClock
        The clock instance.
    base_url : str
        The base URL for the WebSocket connection.
    handler : Callable[[bytes], None]
        The callback handler for message events.
    handler_reconnect : Callable[..., Awaitable[None]], optional
        The callback handler to be called on reconnect.
    is_private : bool, optional
        Whether the client is a private channel.
    is_trade : bool, optional
        Whether the client is a trade channel.
    ws_trade_timeout_secs: PositiveFloat, default 5.0
        The timeout for trade websocket messages.
    recv_window_ms : int, default 5_000
        The receive window (milliseconds) for Bybit WebSocket order requests.

    Raises
    ------
    ValueError
        If `is_private` and `is_trade` are both True.

    """

    def __init__(
        self,
        clock: LiveClock,
        base_url: str,
        handler: Callable[[bytes], None],
        handler_reconnect: Callable[..., Awaitable[None]] | None,
        api_key: str,
        api_secret: str,
        loop: asyncio.AbstractEventLoop,
        is_private: bool | None = False,
        is_trade: bool | None = False,
        ws_trade_timeout_secs: PositiveFloat | None = 5.0,
        ws_auth_timeout_secs: PositiveFloat | None = 5.0,
        recv_window_ms: int = 5_000,
    ) -> None:
        if is_private and is_trade:
            raise ValueError("`is_private` and `is_trade` cannot both be True")

        self._clock = clock
        self._log: Logger = Logger(name=type(self).__name__)

        self._base_url: str = base_url
        self._headers: list[tuple[str, str]] = [
            ("Content-Type", "application/json"),
            ("User-Agent", nautilus_trader.NAUTILUS_USER_AGENT),
            ("Referer", nautilus_pyo3.BYBIT_NAUTILUS_BROKER_ID),
        ]
        self._handler: Callable[[bytes], None] = handler
        self._handler_reconnect: Callable[..., Awaitable[None]] | None = handler_reconnect
        self._loop = loop
        self._ws_trade_timeout_secs = ws_trade_timeout_secs

        self._client: WebSocketClient | None = None
        self._api_key = api_key
        self._api_secret = SecureString(api_secret, name="api_secret")
        self._recv_window_ms: int = recv_window_ms

        self._is_running = False
        self._reconnecting = False

        self._subscriptions: list[str] = []

        self._is_private = is_private
        self._is_trade = is_trade
        self._auth_required = is_private or is_trade
        self._is_authenticated = False
        self._ws_auth_timeout_secs = ws_auth_timeout_secs

        self._tasks: WeakSet[asyncio.Task] = WeakSet()
        self._auth_event = asyncio.Event()

        self._pending_order_requests: dict[str, WsOrderResponseMsgFuture] = {}

        self._decoder_ws_message_general = msgspec_json.Decoder(BybitWsMessageGeneral)
        self._decoder_ws_private_channel_auth = msgspec_json.Decoder(BybitWsPrivateChannelAuthMsg)
        self._decoder_ws_trade_auth = msgspec_json.Decoder(BybitWsTradeAuthMsg)

        # Decoders for WebSocket order response messages
        self._decoder_ws_order_resp_general = msgspec_json.Decoder(BybitWsOrderResponseMsgGeneral)

        self._decoder_ws_order_resp_place = msgspec_json.Decoder(BybitWsPlaceOrderResponseMsg)
        self._decoder_ws_order_resp_amend = msgspec_json.Decoder(BybitWsAmendOrderResponseMsg)
        self._decoder_ws_order_resp_cancel = msgspec_json.Decoder(BybitWsCancelOrderResponseMsg)
        self._decoder_ws_order_resp_batch_place = msgspec_json.Decoder(
            BybitWsBatchPlaceOrderResponseMsg,
        )
        self._decoder_ws_order_resp_batch_amend = msgspec_json.Decoder(
            BybitWsBatchAmendOrderResponseMsg,
        )
        self._decoder_ws_order_resp_batch_cancel = msgspec_json.Decoder(
            BybitWsBatchCancelOrderResponseMsg,
        )
        self._decoder_ws_order_resp_map = {
            BybitWsOrderRequestMsgOP.CREATE: self._decoder_ws_order_resp_place,
            BybitWsOrderRequestMsgOP.AMEND: self._decoder_ws_order_resp_amend,
            BybitWsOrderRequestMsgOP.CANCEL: self._decoder_ws_order_resp_cancel,
            BybitWsOrderRequestMsgOP.CREATE_BATCH: self._decoder_ws_order_resp_batch_place,
            BybitWsOrderRequestMsgOP.AMEND_BATCH: self._decoder_ws_order_resp_batch_amend,
            BybitWsOrderRequestMsgOP.CANCEL_BATCH: self._decoder_ws_order_resp_batch_cancel,
        }

    @property
    def subscriptions(self) -> list[str]:
        return self._subscriptions

    def has_subscription(self, item: str) -> bool:
        return item in self._subscriptions

    @property
    def channel_type(self) -> str:
        if self._is_private:
            return "Private"
        elif self._is_trade:
            return "Trade"
        else:
            return "Public"

    async def connect(self) -> None:
        self._is_running = True
        self._log.debug(f"Connecting to {self._base_url} websocket stream")
        config = WebSocketConfig(
            url=self._base_url,
            handler=self._msg_handler,
            heartbeat=20,
            heartbeat_msg=msgspec_json.encode({"op": "ping"}).decode(),
            headers=self._headers,
        )
        client = await WebSocketClient.connect(
            config=config,
            post_reconnection=self.reconnect,
        )
        self._client = client
        self._log.info(f"Connected to {self._base_url}", LogColor.BLUE)

        # Authenticate
        if self._auth_required:
            await self._authenticate()

    def reconnect(self) -> None:
        """
        Reconnect the client to the server and resubscribe to all streams.
        """
        if not self._is_running or self._reconnecting:
            return

        self._log.warning(f"Trying to reconnect to {self._base_url}")
        self._reconnecting = True
        task = self._loop.create_task(self._reconnect_wrapper())
        self._tasks.add(task)

    async def _reconnect_wrapper(self) -> None:
        try:
            # Authenticate
            if self._auth_required:
                await self._authenticate()

            self._log.warning(
                f"Resubscribing {self.channel_type} channel to {len(self._subscriptions)} streams",
            )

            # Re-subscribe to all streams
            await self._subscribe_all()

            if self._handler_reconnect:
                await self._handler_reconnect()

            self._log.warning(f"Reconnected to {self._base_url}")
        except Exception as e:
            self._log.exception("Reconnection failed", e)
        finally:
            self._reconnecting = False

    async def disconnect(self) -> None:
        self._is_running = False
        self._reconnecting = False

        await cancel_tasks_with_timeout(self._tasks, self._log)

        if self._client is None:
            self._log.warning("Cannot disconnect: not connected")
            return

        try:
            await self._client.disconnect()
        except WebSocketClientError as e:
            self._log.error(str(e))

        self._client = None  # Dispose (will go out of scope)

        self._log.info(f"Disconnected from {self._base_url}", LogColor.BLUE)

    def _msg_handler(self, raw: bytes) -> None:
        """
        Handle pushed websocket messages.

        Parameters
        ----------
        raw : bytes
            The received message in bytes.

        """
        # TODO: better way to improve performance with high message volume?

        msg = self._decoder_ws_message_general.decode(raw)
        op = msg.op

        if self._auth_required and not self._is_authenticated and op == "auth":
            self._check_auth_success(raw)
            return

        if self._is_trade and op and "order." in op:
            self._handle_order_ack(raw)

        self._handler(raw)

    def _check_auth_success(self, raw: bytes) -> None:
        msg: BybitWsPrivateChannelAuthMsg | BybitWsTradeAuthMsg

        if self._is_private:
            msg = self._decoder_ws_private_channel_auth.decode(raw)
        elif self._is_trade:
            msg = self._decoder_ws_trade_auth.decode(raw)
        else:
            raise RuntimeError("Invalid channel type")

        if msg.is_auth_success():
            self._is_authenticated = True
            self._auth_event.set()
            self._log.info(f"{self.channel_type} channel authenticated", LogColor.GREEN)
        else:
            raise RuntimeError(f"{self.channel_type} channel authentication failed: {msg}")

    async def _authenticate(self) -> None:
        self._is_authenticated = False
        self._auth_event.clear()

        signature = self._get_signature()

        try:
            await self._send(signature)
            await asyncio.wait_for(
                self._auth_event.wait(),
                timeout=self._ws_auth_timeout_secs,
            )
        except TimeoutError:
            self._log.warning(f"{self.channel_type} channel authentication timeout")
            raise
        except WebSocketClientError as e:
            self._log.exception(
                f"{self.channel_type} channel failed to send authentication request",
                {e},
            )
            raise
        except Exception as e:
            self._log.exception(
                f"{self.channel_type} channel unexpected error during authentication",
                {e},
            )
            raise

    async def _subscribe(self, subscription: str) -> None:
        self._log.debug(f"Subscribing to {subscription}")
        if subscription in self._subscriptions:
            self._log.warning(f"Cannot subscribe '{subscription}': already subscribed")
            return

        self._subscriptions.append(subscription)
        msg = {"op": "subscribe", "args": [subscription]}
        await self._send(msg)

    async def _unsubscribe(self, subscription: str) -> None:
        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        msg = {"op": "unsubscribe", "args": [subscription]}
        await self._send(msg)

    async def _subscribe_all(self) -> None:
        if self._client is None:
            self._log.error("Cannot subscribe all: not connected")
            return

        # You can input up to 10 args for each subscription request sent to one connection
        subscription_lists = [
            self._subscriptions[i : i + MAX_ARGS_PER_SUBSCRIPTION_REQUEST]
            for i in range(0, len(self._subscriptions), MAX_ARGS_PER_SUBSCRIPTION_REQUEST)
        ]

        for subscriptions in subscription_lists:
            msg = {"op": "subscribe", "args": subscriptions}
            await self._send(msg)

    async def _send(self, msg: dict[str, Any]) -> None:
        await self._send_text(msgspec_json.encode(msg))

    async def _send_text(self, msg: bytes) -> None:
        if self._client is None:
            self._log.error(f"Cannot send message {msg!r}: not connected")
            return

        self._log.debug(f"SENDING: {msg!r}")

        try:
            await self._client.send_text(msg)
        except WebSocketClientError as e:
            self._log.error(str(e))

    ################################################################################
    # Public
    ################################################################################

    async def subscribe_order_book(self, symbol: str, depth: int) -> None:
        subscription = f"orderbook.{depth}.{symbol}"
        await self._subscribe(subscription)

    async def subscribe_trades(self, symbol: str) -> None:
        subscription = f"publicTrade.{symbol}"
        await self._subscribe(subscription)

    async def subscribe_tickers(self, symbol: str) -> None:
        subscription = f"tickers.{symbol}"
        await self._subscribe(subscription)

    async def subscribe_klines(self, symbol: str, interval: str) -> None:
        subscription = f"kline.{interval}.{symbol}"
        await self._subscribe(subscription)

    async def unsubscribe_order_book(self, symbol: str, depth: int) -> None:
        subscription = f"orderbook.{depth}.{symbol}"
        await self._unsubscribe(subscription)

    async def unsubscribe_trades(self, symbol: str) -> None:
        subscription = f"publicTrade.{symbol}"
        await self._unsubscribe(subscription)

    async def unsubscribe_tickers(self, symbol: str) -> None:
        subscription = f"tickers.{symbol}"
        await self._unsubscribe(subscription)

    async def unsubscribe_klines(self, symbol: str, interval: str) -> None:
        subscription = f"kline.{interval}.{symbol}"
        await self._unsubscribe(subscription)

    ################################################################################
    # Private
    ################################################################################

    async def subscribe_account_position_update(self) -> None:
        subscription = "position"
        await self._subscribe(subscription)

    async def subscribe_orders_update(self) -> None:
        subscription = "order"
        await self._subscribe(subscription)

    async def subscribe_executions_update(self) -> None:
        subscription = "execution"
        await self._subscribe(subscription)

    async def subscribe_executions_fast_update(self) -> None:
        subscription = "execution.fast"
        await self._subscribe(subscription)

    async def subscribe_wallet_update(self) -> None:
        subscription = "wallet"
        await self._subscribe(subscription)

    def _get_signature(self):
        expires = self._clock.timestamp_ms() + 5_000
        sign = f"GET/realtime{expires}"
        signature = hmac_signature(self._api_secret.get_value(), sign)
        return {
            "op": "auth",
            "args": [self._api_key, expires, signature],
        }

    ################################################################################
    # Trade
    ################################################################################

    def _handle_order_ack(self, raw: bytes) -> None:
        try:
            msg: BybitWsOrderResponseMsgGeneral = self._decoder_ws_order_resp_general.decode(raw)
        except Exception as e:
            self._log.exception(f"Failed to decode order ack response {raw!r}", e)
            return

        req_id = msg.reqId
        if not req_id:
            self._log.debug(f"No `reqId` in order ack response: {msg}")
            return

        future = self._pending_order_requests.pop(req_id, None)
        if future is not None:
            try:
                order_resp: BybitWsOrderResponseMsg = self._decoder_ws_order_resp_map[msg.op].decode(raw)  # type: ignore[attr-defined]
                if order_resp.retCode == 0:
                    future.set_result(order_resp)
                else:
                    future.set_exception(
                        BybitError(code=order_resp.retCode, message=order_resp.retMsg),
                    )
            except Exception as e:
                self._log.exception(f"Failed to decode order ack response {raw!r}", e)
        else:
            self._log.warning(f"Received ack for `unknown/timeout` reqId={req_id}, msg={msg}")

    async def _order(
        self,
        op: BybitWsOrderRequestMsgOP,
        args: list[
            BybitPlaceOrderPostParams
            | BybitAmendOrderPostParams
            | BybitCancelOrderPostParams
            | BybitBatchPlaceOrderPostParams
            | BybitBatchAmendOrderPostParams
            | BybitBatchCancelOrderPostParams
        ],
        timeout_secs: PositiveFloat | None,
    ) -> BybitWsOrderResponseMsg:
        req_id = UUID4().value

        future: WsOrderResponseMsgFuture = self._loop.create_future()
        self._pending_order_requests[req_id] = future

        # Build request
        request = BybitWsOrderRequestMsg(
            reqId=req_id,
            header={
                "X-BAPI-TIMESTAMP": str(self._clock.timestamp_ms()),
                "X-BAPI-RECV-WINDOW": str(self._recv_window_ms),
                "Referer": nautilus_pyo3.BYBIT_NAUTILUS_BROKER_ID,
            },
            op=op,
            args=args,  # Args array, support one item only for now
        )

        # Send request
        await self._send_text(msgspec_json.encode(request))

        # Wait for response or timeout
        try:
            ack_resp = await asyncio.wait_for(future, timeout_secs)
        except TimeoutError as e:
            self._log.error(f"Order request `{req_id}` timed out. op={op}, args={args}")
            future.cancel()
            raise BybitError(code=-10_408, message="Request timed out") from e

        return ack_resp

    async def place_order(
        self,
        product_type: BybitProductType,
        symbol: str,
        side: BybitOrderSide,
        quantity: str,
        quote_quantity: bool,
        order_type: BybitOrderType,
        price: str | None = None,
        time_in_force: BybitTimeInForce | None = None,
        client_order_id: str | None = None,
        reduce_only: bool | None = None,
        is_leverage: bool | None = None,
        tpsl_mode: BybitTpSlMode | None = None,
        close_on_trigger: bool | None = None,
        tp_order_type: BybitOrderType | None = None,
        sl_order_type: BybitOrderType | None = None,
        trigger_direction: BybitTriggerDirection | None = None,
        trigger_type: BybitTriggerType | None = None,
        trigger_price: str | None = None,
        sl_trigger_price: str | None = None,
        tp_trigger_price: str | None = None,
        tp_limit_price: str | None = None,
        sl_limit_price: str | None = None,
    ) -> BybitWsOrderResponseMsg:
        return await self._order(
            timeout_secs=self._ws_trade_timeout_secs,
            op=BybitWsOrderRequestMsgOP.CREATE,
            args=[
                BybitPlaceOrderPostParams(
                    category=product_type,
                    symbol=symbol,
                    side=side,
                    orderType=order_type,
                    qty=quantity,
                    marketUnit="baseCoin" if not quote_quantity else "quoteCoin",
                    price=price,
                    timeInForce=time_in_force,
                    orderLinkId=client_order_id,
                    isLeverage=int(is_leverage) if is_leverage is not None else None,
                    reduceOnly=reduce_only,
                    closeOnTrigger=close_on_trigger,
                    tpslMode=tpsl_mode if product_type != BybitProductType.SPOT else None,
                    triggerPrice=trigger_price,
                    triggerDirection=trigger_direction,
                    triggerBy=trigger_type,
                    takeProfit=tp_trigger_price if product_type == BybitProductType.SPOT else None,
                    stopLoss=sl_trigger_price if product_type == BybitProductType.SPOT else None,
                    slTriggerBy=trigger_type if product_type != BybitProductType.SPOT else None,
                    tpTriggerBy=trigger_type if product_type != BybitProductType.SPOT else None,
                    tpLimitPrice=tp_limit_price if product_type != BybitProductType.SPOT else None,
                    slLimitPrice=sl_limit_price if product_type != BybitProductType.SPOT else None,
                    tpOrderType=tp_order_type,
                    slOrderType=sl_order_type,
                ),
            ],
        )

    async def amend_order(
        self,
        product_type: BybitProductType,
        symbol: str,
        client_order_id: str | None = None,
        venue_order_id: str | None = None,
        trigger_price: str | None = None,
        quantity: str | None = None,
        price: str | None = None,
    ) -> BybitWsOrderResponseMsg:
        return await self._order(
            timeout_secs=self._ws_trade_timeout_secs,
            op=BybitWsOrderRequestMsgOP.AMEND,
            args=[
                BybitAmendOrderPostParams(
                    category=product_type,
                    symbol=symbol,
                    orderId=venue_order_id,
                    orderLinkId=client_order_id,
                    triggerPrice=trigger_price,
                    qty=quantity,
                    price=price,
                ),
            ],
        )

    async def cancel_order(
        self,
        product_type: BybitProductType,
        symbol: str,
        client_order_id: str | None = None,
        venue_order_id: str | None = None,
        order_filter: str | None = None,
    ) -> BybitWsOrderResponseMsg:
        return await self._order(
            timeout_secs=self._ws_trade_timeout_secs,
            op=BybitWsOrderRequestMsgOP.CANCEL,
            args=[
                BybitCancelOrderPostParams(
                    category=product_type,
                    symbol=symbol,
                    orderId=venue_order_id,
                    orderLinkId=client_order_id,
                    orderFilter=order_filter,
                ),
            ],
        )

    async def batch_place_orders(
        self,
        product_type: BybitProductType,
        submit_orders: list[BybitBatchPlaceOrder],
    ) -> BybitWsOrderResponseMsg:
        return await self._order(
            timeout_secs=self._ws_trade_timeout_secs,
            op=BybitWsOrderRequestMsgOP.CREATE_BATCH,
            args=[
                BybitBatchPlaceOrderPostParams(
                    category=product_type,
                    request=submit_orders,
                ),
            ],
        )

    async def batch_cancel_orders(
        self,
        product_type: BybitProductType,
        cancel_orders: list[BybitBatchCancelOrder],
    ) -> BybitWsOrderResponseMsg:
        return await self._order(
            timeout_secs=self._ws_trade_timeout_secs,
            op=BybitWsOrderRequestMsgOP.CANCEL_BATCH,
            args=[
                BybitBatchCancelOrderPostParams(
                    category=product_type,
                    request=cancel_orders,
                ),
            ],
        )
