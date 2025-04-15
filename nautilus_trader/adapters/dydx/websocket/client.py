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
Provide a dYdX streaming WebSocket client.
"""

import asyncio
from collections import defaultdict
from collections.abc import Awaitable
from collections.abc import Callable
from typing import Any

import msgspec

from nautilus_trader.adapters.dydx.common.enums import DYDXCandlesResolution
from nautilus_trader.adapters.dydx.common.enums import DYDXChannel
from nautilus_trader.adapters.dydx.http.errors import should_retry
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsMessageGeneral
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.nautilus_pyo3 import Quota
from nautilus_trader.core.nautilus_pyo3 import WebSocketClient
from nautilus_trader.core.nautilus_pyo3 import WebSocketClientError
from nautilus_trader.core.nautilus_pyo3 import WebSocketConfig
from nautilus_trader.live.retry import RetryManagerPool


class DYDXWebsocketClient:
    """
    Provide a dYdX streaming WebSocket client.

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
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    subscription_rate_limit_per_second : int, default 2
        The maximum number of subscription message to send to the venue.
    max_send_retries : int, optional
        Maximum retries when sending websocket messages.
    retry_delay_secs : float, optional
        The delay (seconds) between retry attempts when resending websocket messages.

    """

    def __init__(
        self,
        clock: LiveClock,
        base_url: str,
        handler: Callable[[bytes], None],
        handler_reconnect: Callable[..., Awaitable[None]] | None,
        loop: asyncio.AbstractEventLoop,
        subscription_rate_limit_per_second: int = 2,
        max_send_retries: int | None = None,
        retry_delay_secs: float | None = None,
    ) -> None:
        """
        Provide a dYdX streaming WebSocket client.
        """
        self._clock = clock
        self._log: Logger = Logger(name=type(self).__name__)
        self._base_url: str = base_url
        self._handler: Callable[[bytes], None] = handler
        self._handler_reconnect: Callable[..., Awaitable[None]] | None = handler_reconnect
        self._loop = loop
        self._client: WebSocketClient | None = None
        self._is_running = False
        self._subscriptions: dict[DYDXChannel, set[str | None]] = defaultdict(set)
        self._subscription_rate_limit_per_second = subscription_rate_limit_per_second
        self._max_send_retries = max_send_retries
        self._retry_delay_secs = retry_delay_secs
        self._decoder_ws_msg_general = msgspec.json.Decoder(DYDXWsMessageGeneral)

    def is_connected(self) -> bool:
        """
        Return whether the client is connected.

        Returns
        -------
        bool

        """
        return self._client is not None and self._client.is_active()

    def is_disconnected(self) -> bool:
        """
        Return whether the client is disconnected.

        Returns
        -------
        bool

        """
        return not self.is_connected()

    @property
    def subscriptions(self) -> dict[DYDXChannel, set[str | None]]:
        """
        Return the dictionary of subscriptions.

        Returns
        -------
        dict[DYDXChannel, set[str | None]]
            Dictionary of subscriptions.

        """
        return self._subscriptions

    def has_subscription(self, channel: DYDXChannel, channel_id: str | None = None) -> bool:
        """
        Return true if the connection is already subscribed to this topic.

        Parameters
        ----------
        channel : DYDXChannel
            DYDXChannel enum.
        channel_id : str, optional
            Unique channel id.

        Returns
        -------
        bool
            Whether the client is already subscribed to this topic.

        """
        subscriptions_per_channel = self._subscriptions.get(channel)

        if subscriptions_per_channel is None:
            return False

        return channel_id in subscriptions_per_channel

    async def connect(self) -> None:
        """
        Connect to the websocket server.
        """
        self._is_running = True
        self._retry_manager_pool = RetryManagerPool[None](
            pool_size=100,
            max_retries=self._max_send_retries or 0,
            retry_delay_secs=self._retry_delay_secs or 1.0,
            logger=self._log,
            exc_types=(WebSocketClientError,),
            retry_check=should_retry,
        )

        self._log.debug(f"Connecting to {self._base_url} websocket stream")
        config = WebSocketConfig(
            url=self._base_url,
            handler=self._handle_msg,
            heartbeat=10,
            headers=[],
            ping_handler=self._handle_ping,
        )
        client = await WebSocketClient.connect(
            config=config,
            post_reconnection=self.reconnect,
            default_quota=Quota.rate_per_second(self._subscription_rate_limit_per_second),
        )
        self._client = client
        self._log.info(f"Connected to {self._base_url}", LogColor.BLUE)

    def _handle_msg(self, raw: bytes) -> None:
        """
        Handle websocket messages.

        In case an internal error at the venue occurs, the client tries
        to resubscribe to the channel. Otherwise, the message is passed
        as is to the subscriber.

        Parameters
        ----------
        raw : bytes
            The received message in bytes.

        """
        ws_message = self._decoder_ws_msg_general.decode(raw)

        if (
            ws_message.type == "error"
            and ws_message.message is not None
            and ws_message.channel is not None
            and ws_message.message.startswith(
                "Internal error, could not fetch data for subscription:",
            )
        ):
            msg = {"type": "subscribe", "channel": ws_message.channel}

            if ws_message.id is not None:
                msg["id"] = ws_message.id

            self._log.warning(
                f"{ws_message.message} Resubscribe to channel {ws_message.channel} id {ws_message.id}",
            )
            self._loop.create_task(self._send(msg, delay_secs=1.0))

            # Do not handle this message with the client handler.
            # The error is already handled by resubscribing to the channel
            return

        self._handler(raw)

    def _handle_ping(self, raw: bytes) -> None:
        """
        Handle ping messages by returning a pong message.

        Parameters
        ----------
        raw : bytes
            The received ping in bytes.

        """
        self._loop.create_task(self.send_pong(raw))

    async def send_pong(self, raw: bytes) -> None:
        """
        Send the given raw payload to the server as a PONG message.

        Parameters
        ----------
        raw : bytes
            The pong message in bytes.

        """
        if self._client is None or self._client.is_active() is False:
            return

        async with self._retry_manager_pool as retry_manager:
            await retry_manager.run(
                name="send_pong",
                details=[raw],
                func=self._client.send_pong,
                data=raw,
            )

    def reconnect(self) -> None:
        """
        Reconnect the client to the server and resubscribe to all streams.
        """
        if not self._is_running:
            return

        self._log.warning(f"Reconnected to {self._base_url}")

        # Re-subscribe to all streams
        self._loop.create_task(self._subscribe_all())

        if self._handler_reconnect:
            self._loop.create_task(self._handler_reconnect())  # type: ignore

    async def disconnect(self) -> None:
        """
        Close the websocket connection.
        """
        self._is_running = False

        if self._client is None:
            self._log.warning("Cannot disconnect: not connected")
            return

        try:
            await self._client.disconnect()
        except WebSocketClientError as e:
            self._log.error(f"Failed to close websocket connection: {e}")

        self._client = None  # Dispose (will go out of scope)

        self._retry_manager_pool.shutdown()

        self._log.info(f"Disconnected from {self._base_url}", LogColor.BLUE)

    async def subscribe_trades(self, symbol: str) -> None:
        """
        Subscribe to trades messages.

        Parameters
        ----------
        symbol : str
            Symbol of the instrument to subscribe to.

        """
        await self.subscribe_channel(channel=DYDXChannel.TRADES, channel_id=symbol)

    async def subscribe_order_book(self, symbol: str) -> None:
        """
        Subscribe to order book messages.

        Parameters
        ----------
        symbol : str
            Symbol of the instrument to subscribe to.

        """
        await self.subscribe_channel(channel=DYDXChannel.ORDERBOOK, channel_id=symbol)

    async def subscribe_klines(self, symbol: str, interval: DYDXCandlesResolution) -> None:
        """
        Subscribe to klines.

        Parameters
        ----------
        symbol : str
            Symbol of the instrument to subscribe to bars.
        interval : DYDXCandlesResolution
            Specify the interval between candle updates (for example 1MIN).

        """
        await self.subscribe_channel(
            channel=DYDXChannel.CANDLES,
            channel_id=f"{symbol}/{interval.value}",
        )

    async def subscribe_markets(self) -> None:
        """
        Subscribe to instrument updates.
        """
        await self.subscribe_channel(channel=DYDXChannel.MARKETS)

    async def subscribe_account_update(self, wallet_address: str, subaccount_number: int) -> None:
        """
        Subscribe to realtime information about orders, fills, transfers, perpetual
        positions, and perpetual assets for a subaccount.

        Parameters
        ----------
        wallet_address : str
            The dYdX wallet address.
        subaccount_number : int
            The subaccount number.
            The venue creates subaccount 0 by default.

        """
        await self.subscribe_channel(
            channel=DYDXChannel.SUBACCOUNTS,
            channel_id=f"{wallet_address}/{subaccount_number}",
        )

    async def subscribe_block_height(self) -> None:
        """
        Subscribe to block height messages.
        """
        await self.subscribe_channel(channel=DYDXChannel.BLOCK_HEIGHT)

    async def subscribe_channel(self, channel: DYDXChannel, channel_id: str | None = None) -> None:
        """
        Subscribe to a websocket channel.

        Parameters
        ----------
        channel : DYDXChannel
            The channel enum.
        channel_id : str, optional
            Channel id for a specific instrument.

        """
        if self._client is None:
            self._log.warning(f"Cannot subscribe to {channel.value}: not connected")
            return

        if self.has_subscription(channel=channel, channel_id=channel_id):
            self._log.warning(
                f"Cannot subscribe '{channel.value} for {channel_id}': already subscribed",
            )
            return

        self._subscriptions[channel].add(channel_id)
        msg = {"type": "subscribe", "channel": channel.value}

        if channel_id is not None:
            msg["id"] = channel_id

        self._log.debug(f"Subscribe to {channel.value}")
        await self._send(msg)

    async def unsubscribe_channel(
        self,
        channel: DYDXChannel,
        channel_id: str | None = None,
    ) -> None:
        """
        Unsubscribe from a websocket channel.

        Parameters
        ----------
        channel : DYDXChannel
            The channel enum.
        channel_id : str, optional
            Channel id for a specific instrument.

        """
        if self._client is None:
            self._log.warning(f"Cannot unsubscribe from {channel.value}: not connected")
            return

        if not self.has_subscription(channel=channel, channel_id=channel_id):
            self._log.warning(
                f"Cannot unsubscribe '{channel.value} for {channel_id}': not subscribed",
            )
            return

        if channel_id is None:
            if len(self._subscriptions[channel]) > 1:
                self._log.error(
                    f"Cannot unsubscribe from {channel.value}: multiple subscriptions exist, specify channel_id",
                )
                return

            self._subscriptions.pop(channel)
        else:
            self._subscriptions[channel].remove(channel_id)

        msg = {"type": "unsubscribe", "channel": channel.value}

        if channel_id is not None:
            msg["id"] = channel_id

        self._log.debug(f"Unsubscribe from {channel.value}")
        await self._send(msg)

    async def unsubscribe_account_update(self, wallet_address: str, subaccount_number: int) -> None:
        """
        Unsubscribe from account updates.

        Parameters
        ----------
        wallet_address : str
            The dYdX wallet address.
        subaccount_number : int
            The subaccount number.
            The venue creates subaccount 0 by default.

        """
        await self.unsubscribe_channel(
            channel=DYDXChannel.SUBACCOUNTS,
            channel_id=f"{wallet_address}/{subaccount_number}",
        )

    async def unsubscribe_trades(self, symbol: str) -> None:
        """
        Unsubscribe from trades messages.

        Parameters
        ----------
        symbol : str
            Symbol of the instrument to unsubscribe from.

        """
        await self.unsubscribe_channel(channel=DYDXChannel.TRADES, channel_id=symbol)

    async def unsubscribe_order_book(self, symbol: str) -> None:
        """
        Unsubscribe from order book messages.

        Parameters
        ----------
        symbol : str
            Symbol of the instrument to unsubscribe from.

        """
        await self.unsubscribe_channel(channel=DYDXChannel.ORDERBOOK, channel_id=symbol)

    async def unsubscribe_klines(self, symbol: str, interval: DYDXCandlesResolution) -> None:
        """
        Unsubscribe from bar messages.

        Parameters
        ----------
        symbol : str
            Symbol of the instrument to unsubscribe from bars.
        interval : DYDXCandlesResolution
            Specify the interval between candle updates (for example 1MIN).

        """
        await self.unsubscribe_channel(
            channel=DYDXChannel.CANDLES,
            channel_id=f"{symbol}/{interval.value}",
        )

    async def unsubscribe_markets(self) -> None:
        """
        Unsubscribe from market updates.
        """
        await self.unsubscribe_channel(channel=DYDXChannel.MARKETS)

    async def unsubscribe_block_height(self) -> None:
        """
        Unsubscribe from block height updates.
        """
        await self.unsubscribe_channel(channel=DYDXChannel.BLOCK_HEIGHT)

    async def _subscribe_all(self) -> None:
        """
        Resubscribe to all previous subscriptions.
        """
        if self._client is None:
            self._log.error("Cannot subscribe all: not connected")
            return

        for dydx_channel, channel_ids in self._subscriptions.items():
            for channel_id in channel_ids:
                msg: dict[str, Any] = {
                    "type": "subscribe",
                    "channel": dydx_channel.value,
                }

                if channel_id is not None:
                    msg["id"] = channel_id

                await self._send(msg)

                # Delay due to rate limiting
                # 2 subscriptions per (connection + channel + channel id) per second
                await asyncio.sleep(delay=0.5)

    async def _send(self, msg: dict[str, Any], delay_secs: float = 0.0) -> None:
        """
        Send a message to the venue.

        Parameters
        ----------
        msg : dict[str, Any]
            Dictionary to serialize as JSON message and send
        delay_secs : float, default 0.0
            Delay in seconds before sending the message.

        """
        if self._client is None:
            self._log.error(f"Cannot send message {msg}: not connected")
            return

        if delay_secs > 0.0:
            await asyncio.sleep(delay=delay_secs)

        self._log.debug(f"SENDING: {msg}")

        data = msgspec.json.encode(msg)

        async with self._retry_manager_pool as retry_manager:
            await retry_manager.run(
                name="send_text",
                details=[data],
                func=self._client.send_text,
                data=data,
            )
