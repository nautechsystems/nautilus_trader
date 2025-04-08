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

from msgspec import json as msgspec_json

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
from nautilus_trader.adapters.bybit.endpoints.trade.cancel_order import BybitCancelOrderPostParams
from nautilus_trader.adapters.bybit.endpoints.trade.place_order import BybitPlaceOrderPostParams
from nautilus_trader.adapters.bybit.http.errors import BybitError
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsMessageGeneral
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsOrderRequestMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsOrderResponseMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsPrivateChannelAuthMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsTradeAuthMsg
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.nautilus_pyo3 import WebSocketClient
from nautilus_trader.core.nautilus_pyo3 import WebSocketClientError
from nautilus_trader.core.nautilus_pyo3 import WebSocketConfig
from nautilus_trader.core.nautilus_pyo3 import hmac_signature
from nautilus_trader.core.uuid import UUID4


if TYPE_CHECKING:
    from collections.abc import Awaitable
    from collections.abc import Callable
    from collections.abc import Coroutine
    from typing import Any

    from nautilus_trader.adapters.bybit.endpoints.trade.batch_cancel_order import BybitBatchCancelOrder
    from nautilus_trader.adapters.bybit.endpoints.trade.batch_place_order import BybitBatchPlaceOrder

# fmt: on

MAX_ARGS_PER_SUBSCRIPTION_REQUEST = 10

WsOrderResponseFuture = asyncio.Future[BybitWsOrderResponseMsg]


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
    ws_trade_timeout_secs: float, default 5.0
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
        ws_trade_timeout_secs: float | None = 5.0,
        recv_window_ms: int = 5_000,
    ) -> None:
        if is_private and is_trade:
            raise ValueError("`is_private` and `is_trade` cannot both be True")

        self._clock = clock
        self._log: Logger = Logger(name=type(self).__name__)

        self._base_url: str = base_url
        self._handler: Callable[[bytes], None] = handler
        self._handler_reconnect: Callable[..., Awaitable[None]] | None = handler_reconnect
        self._loop = loop
        self._ws_trade_timeout_secs = ws_trade_timeout_secs

        self._client: WebSocketClient | None = None
        self._api_key = api_key
        self._api_secret = api_secret
        self._recv_window_ms: int = recv_window_ms

        self._is_running = False
        self._reconnecting = False

        self._subscriptions: list[str] = []

        self._is_private = is_private
        self._is_trade = is_trade
        self._auth_required = is_private or is_trade
        self._is_authenticated = False

        self._decoder_ws_message_general = msgspec_json.Decoder(BybitWsMessageGeneral)
        self._decoder_ws_private_channel_auth = msgspec_json.Decoder(BybitWsPrivateChannelAuthMsg)
        self._decoder_ws_trade_auth = msgspec_json.Decoder(BybitWsTradeAuthMsg)
        self._decoder_ws_order_response = msgspec_json.Decoder(BybitWsOrderResponseMsg)

        self._pending_order_requests: dict[str, WsOrderResponseFuture] = {}

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
            headers=[],
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
        self._loop.create_task(self._reconnect_wrapper())

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
            self._log.info(f"{self.channel_type} channel authenticated", LogColor.GREEN)
        else:
            raise RuntimeError(f"{self.channel_type} channel authentication failed: {msg}")

    async def _authenticate(self) -> None:
        self._is_authenticated = False
        signature = self._get_signature()
        await self._send(signature)

        while not self._is_authenticated:
            self._log.debug(f"Waiting for {self.channel_type} channel authentication")
            await asyncio.sleep(0.1)

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
        signature = hmac_signature(self._api_secret, sign)
        return {
            "op": "auth",
            "args": [self._api_key, expires, signature],
        }

    ################################################################################
    # Trade
    ################################################################################

    def _handle_order_ack(self, raw: bytes) -> None:
        try:
            msg = self._decoder_ws_order_response.decode(raw)
        except Exception as e:
            self._log.exception(f"Failed to decode order ack response {raw!r}", e)
            return

        req_id = msg.reqId
        if not req_id:
            self._log.debug(f"No `reqId` in order ack response: {msg}")
            return

        ret_code = msg.retCode
        future = self._pending_order_requests.pop(req_id, None)
        if future is not None:
            if ret_code == 0:
                future.set_result(msg)
            else:
                future.set_exception(BybitError(code=ret_code, message=msg.retMsg))
        else:
            self._log.warning(f"Received ack for `unknown/timeout` reqId={req_id}, msg={msg}")
            return

    async def _order(
        self,
        op: BybitWsOrderRequestMsgOP,
        args: list[
            BybitPlaceOrderPostParams | BybitAmendOrderPostParams | BybitCancelOrderPostParams
        ],
        timeout_secs: float | None,
    ) -> BybitWsOrderResponseMsg:
        req_id = UUID4().value

        future: WsOrderResponseFuture = self._loop.create_future()
        self._pending_order_requests[req_id] = future

        # Build request
        request = BybitWsOrderRequestMsg(
            reqId=req_id,
            header={
                "X-BAPI-TIMESTAMP": str(self._clock.timestamp_ms()),
                "X-BAPI-RECV-WINDOW": str(self._recv_window_ms),
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
            self._pending_order_requests.pop(req_id, None)
            raise BybitError(code=-10_408, message="Request timed out") from e

        return ack_resp

    async def _batch_orders(
        self,
        tasks: list[Coroutine[Any, Any, BybitWsOrderResponseMsg]],
    ) -> list[BybitWsOrderResponseMsg]:
        futures = await asyncio.gather(*tasks, return_exceptions=True)

        results: list[BybitWsOrderResponseMsg] = []
        for result in futures:
            if isinstance(result, BybitWsOrderResponseMsg):
                results.append(result)
            else:
                self._log.error(f"Batch orders error: {result}")
        return results

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
    ) -> list[BybitWsOrderResponseMsg]:
        tasks = [
            self.place_order(
                product_type=product_type,
                symbol=order.symbol,
                side=order.side,
                order_type=order.orderType,
                quantity=order.qty,
                quote_quantity=order.marketUnit == "quoteCoin",
                price=order.price,
                time_in_force=order.timeInForce,
                client_order_id=order.orderLinkId,
                reduce_only=order.reduceOnly,
                close_on_trigger=order.closeOnTrigger,
                trigger_price=order.triggerPrice,
                trigger_direction=order.triggerDirection,
                tp_order_type=order.tpOrderType,
                sl_order_type=order.slOrderType,
                tpsl_mode=order.tpslMode,
                tp_trigger_price=order.takeProfit,
                sl_trigger_price=order.stopLoss,
                trigger_type=order.triggerBy,
                tp_limit_price=order.tpLimitPrice,
                sl_limit_price=order.slLimitPrice,
            )
            for order in submit_orders
        ]

        return await self._batch_orders(tasks)

    async def batch_cancel_orders(
        self,
        product_type: BybitProductType,
        cancel_orders: list[BybitBatchCancelOrder],
    ) -> list[BybitWsOrderResponseMsg]:
        tasks = [
            self.cancel_order(
                product_type=product_type,
                symbol=order.symbol,
                client_order_id=order.orderLinkId,
                venue_order_id=order.orderId,
            )
            for order in cancel_orders
        ]

        return await self._batch_orders(tasks)
