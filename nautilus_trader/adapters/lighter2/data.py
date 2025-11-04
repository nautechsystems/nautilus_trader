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
from typing import Any

from nautilus_trader.adapters.lighter2.config import LighterDataClientConfig
from nautilus_trader.adapters.lighter2.constants import LIGHTER_CLIENT_ID
from nautilus_trader.adapters.lighter2.constants import LIGHTER_VENUE
from nautilus_trader.adapters.lighter2.http import LighterHttpClient
from nautilus_trader.adapters.lighter2.providers import LighterInstrumentProvider
from nautilus_trader.adapters.lighter2.websocket import LighterWebSocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.msgbus.bus import MessageBus


class LighterDataClient(LiveMarketDataClient):
    """
    Provides a data client for the Lighter exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : LighterHttpClient
        The Lighter HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.
    config : LighterDataClientConfig
        The configuration for the client.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: LighterHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        config: LighterDataClientConfig,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=LIGHTER_CLIENT_ID,
            venue=LIGHTER_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )

        self._client = client
        self._config = config

        # WebSocket client
        self._ws_client: LighterWebSocketClient | None = None

        # Instrument provider
        self._instrument_provider = LighterInstrumentProvider(
            client=client,
            logger=logger,
            account_type=config.account_type,
        )

        # Subscription tracking
        self._subscribed_instruments: set[InstrumentId] = set()
        self._subscribed_order_books: set[InstrumentId] = set()
        self._subscribed_trades: set[InstrumentId] = set()

    async def _connect(self) -> None:
        """Connect the client."""
        # Connect HTTP client
        await self._client.connect()

        # Initialize WebSocket client
        self._ws_client = LighterWebSocketClient(
            loop=self._loop,
            clock=self._clock,
            logger=self._log,
            handler=self._handle_ws_message,
            api_key_private_key=self._config.api_key_private_key,
            eth_private_key=self._config.eth_private_key,
            base_url=self._config.base_url_ws,
            is_testnet=self._config.is_testnet,
            proxy_url=self._config.ws_proxy_url,
        )

        # Connect WebSocket client
        await self._ws_client.connect()

        # Load instruments
        await self._instrument_provider.load_all_async()

        self._log.info("Lighter data client connected")

    async def _disconnect(self) -> None:
        """Disconnect the client."""
        # Disconnect WebSocket client
        if self._ws_client:
            await self._ws_client.disconnect()
            self._ws_client = None

        # Disconnect HTTP client
        await self._client.disconnect()

        # Clear subscriptions
        self._subscribed_instruments.clear()
        self._subscribed_order_books.clear()
        self._subscribed_trades.clear()

        self._log.info("Lighter data client disconnected")

    def _handle_ws_message(self, raw: bytes) -> None:
        """Handle WebSocket message."""
        self._loop.create_task(self._process_ws_message(raw))

    async def _process_ws_message(self, raw: bytes) -> None:
        """Process WebSocket message."""
        try:
            message_str = raw.decode('utf-8')
            message = json.loads(message_str)

            channel = message.get("channel")
            instrument_id_str = message.get("instrument_id")

            if not instrument_id_str:
                return

            instrument_id = InstrumentId.from_str(f"{instrument_id_str}.{LIGHTER_VENUE}")
            
            if channel == "orderbook":
                await self._handle_orderbook_update(instrument_id, message)
            elif channel == "trades":
                await self._handle_trades_update(instrument_id, message)
            else:
                self._log.debug(f"Unhandled channel: {channel}")

        except Exception as e:
            self._log.error(f"Error processing WebSocket message: {e}")

    async def _handle_orderbook_update(self, instrument_id: InstrumentId, message: dict[str, Any]) -> None:
        """Handle order book update."""
        try:
            data = message.get("data", {})
            bids = data.get("bids", [])
            asks = data.get("asks", [])
            timestamp_ns = self._clock.timestamp_ns()

            # Get instrument
            instrument = self._cache.instrument(instrument_id)
            if not instrument:
                self._log.warning(f"Instrument not found: {instrument_id}")
                return

            # Create order book deltas
            deltas = []

            # Process bids
            for bid in bids:
                price = Price.from_str(str(bid[0]))
                size = Quantity.from_str(str(bid[1]))
                action = BookAction.UPDATE if size > 0 else BookAction.DELETE
                
                delta = OrderBookDelta(
                    instrument_id=instrument_id,
                    action=action,
                    order=None,  # Market data doesn't have order info
                    side=OrderSide.BUY,
                    price=price,
                    size=size,
                    order_id=None,
                    flags=0,
                    sequence=0,
                    ts_event=timestamp_ns,
                    ts_init=timestamp_ns,
                )
                deltas.append(delta)

            # Process asks
            for ask in asks:
                price = Price.from_str(str(ask[0]))
                size = Quantity.from_str(str(ask[1]))
                action = BookAction.UPDATE if size > 0 else BookAction.DELETE
                
                delta = OrderBookDelta(
                    instrument_id=instrument_id,
                    action=action,
                    order=None,
                    side=OrderSide.SELL,
                    price=price,
                    size=size,
                    order_id=None,
                    flags=0,
                    sequence=0,
                    ts_event=timestamp_ns,
                    ts_init=timestamp_ns,
                )
                deltas.append(delta)

            if deltas:
                order_book_deltas = OrderBookDeltas(
                    instrument_id=instrument_id,
                    deltas=deltas,
                    flags=0,
                    sequence=0,
                    ts_event=timestamp_ns,
                    ts_init=timestamp_ns,
                )
                self._handle_data(order_book_deltas)

        except Exception as e:
            self._log.error(f"Error handling orderbook update: {e}")

    async def _handle_trades_update(self, instrument_id: InstrumentId, message: dict[str, Any]) -> None:
        """Handle trades update."""
        try:
            trades = message.get("data", [])
            
            # Get instrument
            instrument = self._cache.instrument(instrument_id)
            if not instrument:
                self._log.warning(f"Instrument not found: {instrument_id}")
                return

            for trade in trades:
                trade_id = TradeId(str(trade.get("id", 0)))
                price = Price.from_str(str(trade.get("price", 0)))
                size = Quantity.from_str(str(trade.get("size", 0)))
                side = OrderSide.BUY if trade.get("side") == "buy" else OrderSide.SELL
                
                # Convert timestamp
                timestamp_ms = trade.get("timestamp", 0)
                timestamp_ns = timestamp_ms * 1_000_000 if timestamp_ms else self._clock.timestamp_ns()

                trade_tick = TradeTick(
                    instrument_id=instrument_id,
                    price=price,
                    size=size,
                    aggressor_side=side,
                    trade_id=trade_id,
                    ts_event=timestamp_ns,
                    ts_init=self._clock.timestamp_ns(),
                )
                
                self._handle_data(trade_tick)

        except Exception as e:
            self._log.error(f"Error handling trades update: {e}")

    async def _subscribe_order_book_deltas(self, instrument_id: InstrumentId) -> None:
        """Subscribe to order book deltas."""
        if not self._ws_client:
            self._log.error("WebSocket client not connected")
            return

        if instrument_id in self._subscribed_order_books:
            self._log.warning(f"Already subscribed to order book for {instrument_id}")
            return

        try:
            # Extract symbol from instrument ID
            symbol = instrument_id.symbol.value.replace("-PERP", "")
            
            await self._ws_client.subscribe_orderbook(symbol, depth=20)
            self._subscribed_order_books.add(instrument_id)
            self._log.info(f"Subscribed to order book deltas for {instrument_id}")

        except Exception as e:
            self._log.error(f"Error subscribing to order book deltas for {instrument_id}: {e}")

    async def _subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        """Subscribe to trade ticks."""
        if not self._ws_client:
            self._log.error("WebSocket client not connected")
            return

        if instrument_id in self._subscribed_trades:
            self._log.warning(f"Already subscribed to trades for {instrument_id}")
            return

        try:
            # Extract symbol from instrument ID
            symbol = instrument_id.symbol.value.replace("-PERP", "")
            
            await self._ws_client.subscribe_trades(symbol)
            self._subscribed_trades.add(instrument_id)
            self._log.info(f"Subscribed to trade ticks for {instrument_id}")

        except Exception as e:
            self._log.error(f"Error subscribing to trade ticks for {instrument_id}: {e}")

    async def _unsubscribe_order_book_deltas(self, instrument_id: InstrumentId) -> None:
        """Unsubscribe from order book deltas."""
        if not self._ws_client:
            self._log.error("WebSocket client not connected")
            return

        if instrument_id not in self._subscribed_order_books:
            self._log.warning(f"Not subscribed to order book for {instrument_id}")
            return

        try:
            # Extract symbol from instrument ID
            symbol = instrument_id.symbol.value.replace("-PERP", "")
            
            await self._ws_client.unsubscribe("orderbook", symbol)
            self._subscribed_order_books.discard(instrument_id)
            self._log.info(f"Unsubscribed from order book deltas for {instrument_id}")

        except Exception as e:
            self._log.error(f"Error unsubscribing from order book deltas for {instrument_id}: {e}")

    async def _unsubscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        """Unsubscribe from trade ticks."""
        if not self._ws_client:
            self._log.error("WebSocket client not connected")
            return

        if instrument_id not in self._subscribed_trades:
            self._log.warning(f"Not subscribed to trades for {instrument_id}")
            return

        try:
            # Extract symbol from instrument ID
            symbol = instrument_id.symbol.value.replace("-PERP", "")
            
            await self._ws_client.unsubscribe("trades", symbol)
            self._subscribed_trades.discard(instrument_id)
            self._log.info(f"Unsubscribed from trade ticks for {instrument_id}")

        except Exception as e:
            self._log.error(f"Error unsubscribing from trade ticks for {instrument_id}: {e}")

    # Required abstract method implementations
    async def _subscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        """Subscribe to quote ticks (not directly supported, use order book)."""
        self._log.warning(f"Quote ticks not directly supported for {instrument_id}, using order book")
        await self._subscribe_order_book_deltas(instrument_id)

    async def _unsubscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        """Unsubscribe from quote ticks."""
        await self._unsubscribe_order_book_deltas(instrument_id)

    # Additional data subscription methods can be added here as needed