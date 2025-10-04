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
"""Data client for Coinbase."""

import asyncio
import json
from decimal import Decimal

import nautilus_pyo3
import pandas as pd
from nautilus_trader.adapters.coinbase.constants import COINBASE_VENUE
from nautilus_trader.adapters.coinbase.providers import CoinbaseInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.datetime import secs_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class CoinbaseDataClient(LiveMarketDataClient):
    """
    Provides a data client for the Coinbase Advanced Trade API.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.CoinbaseHttpClient
        The Coinbase HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : CoinbaseInstrumentProvider
        The instrument provider.
    ws_client : nautilus_pyo3.CoinbaseWebSocketClient, optional
        The WebSocket client for real-time data.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.CoinbaseHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: CoinbaseInstrumentProvider,
        ws_client: nautilus_pyo3.CoinbaseWebSocketClient | None = None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(COINBASE_VENUE.value),
            venue=COINBASE_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        self._client = client
        self._ws_client = ws_client
        self._update_instrument_interval: asyncio.TimerHandle | None = None
        
        # WebSocket message handling task
        self._ws_task: asyncio.Task | None = None

    async def _connect(self) -> None:
        """Connect to Coinbase."""
        self._log.info("Connecting to Coinbase...")

        # Load instruments
        await self._instrument_provider.load_all_async()
        self._log.info(f"Loaded {len(self._instrument_provider.list_all())} instruments")

        # Connect WebSocket if available
        if self._ws_client:
            await self._ws_client.connect()
            self._ws_task = self._loop.create_task(self._handle_ws_messages())
            self._log.info("WebSocket connected")

        self._log.info("Connected to Coinbase")

    async def _disconnect(self) -> None:
        """Disconnect from Coinbase."""
        self._log.info("Disconnecting from Coinbase...")

        # Cancel WebSocket task
        if self._ws_task and not self._ws_task.done():
            self._ws_task.cancel()
            try:
                await self._ws_task
            except asyncio.CancelledError:
                pass

        # Disconnect WebSocket
        if self._ws_client:
            await self._ws_client.disconnect()
            self._log.info("WebSocket disconnected")

        self._log.info("Disconnected from Coinbase")

    async def _handle_ws_messages(self) -> None:
        """Handle incoming WebSocket messages."""
        try:
            while True:
                message = await self._ws_client.receive_message()
                if message is None:
                    self._log.warning("WebSocket connection closed")
                    break

                try:
                    data = json.loads(message)
                    await self._handle_ws_message(data)
                except json.JSONDecodeError as e:
                    self._log.error(f"Failed to decode WebSocket message: {e}")
                except Exception as e:
                    self._log.error(f"Error handling WebSocket message: {e}")

        except asyncio.CancelledError:
            self._log.debug("WebSocket message handler cancelled")
        except Exception as e:
            self._log.error(f"WebSocket message handler error: {e}")

    async def _handle_ws_message(self, data: dict) -> None:
        """Handle a parsed WebSocket message."""
        channel = data.get("channel")
        
        if channel == "ticker":
            await self._handle_ticker(data)
        elif channel == "level2":
            await self._handle_level2(data)
        elif channel == "market_trades":
            await self._handle_market_trades(data)
        elif channel == "heartbeats":
            self._log.debug("Received heartbeat")
        elif channel == "subscriptions":
            self._log.info(f"Subscription update: {data}")
        else:
            self._log.debug(f"Unhandled channel: {channel}")

    async def _handle_ticker(self, data: dict) -> None:
        """Handle ticker updates."""
        # Ticker handling would go here
        pass

    async def _handle_level2(self, data: dict) -> None:
        """Handle order book updates."""
        # Order book handling would go here
        pass

    async def _handle_market_trades(self, data: dict) -> None:
        """Handle trade updates."""
        # Trade handling would go here
        pass

    async def _subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        """Subscribe to trade ticks."""
        if not self._ws_client:
            self._log.warning("WebSocket client not available")
            return

        product_id = instrument_id.symbol.value.replace("/", "-")
        await self._ws_client.subscribe([product_id], "market_trades", False)
        self._log.info(f"Subscribed to trades for {instrument_id}")

    async def _subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type,
        depth: int | None = None,
        kwargs: dict | None = None,
    ) -> None:
        """Subscribe to order book deltas."""
        if not self._ws_client:
            self._log.warning("WebSocket client not available")
            return

        product_id = instrument_id.symbol.value.replace("/", "-")
        await self._ws_client.subscribe([product_id], "level2", False)
        self._log.info(f"Subscribed to order book for {instrument_id}")

    async def _subscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        """Subscribe to quote ticks."""
        if not self._ws_client:
            self._log.warning("WebSocket client not available")
            return

        product_id = instrument_id.symbol.value.replace("/", "-")
        await self._ws_client.subscribe([product_id], "ticker", False)
        self._log.info(f"Subscribed to ticker for {instrument_id}")

    async def _unsubscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        """Unsubscribe from trade ticks."""
        if not self._ws_client:
            return

        product_id = instrument_id.symbol.value.replace("/", "-")
        await self._ws_client.unsubscribe([product_id], "market_trades")
        self._log.info(f"Unsubscribed from trades for {instrument_id}")

    async def _unsubscribe_order_book_deltas(self, instrument_id: InstrumentId) -> None:
        """Unsubscribe from order book deltas."""
        if not self._ws_client:
            return

        product_id = instrument_id.symbol.value.replace("/", "-")
        await self._ws_client.unsubscribe([product_id], "level2")
        self._log.info(f"Unsubscribed from order book for {instrument_id}")

    async def _unsubscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        """Unsubscribe from quote ticks."""
        if not self._ws_client:
            return

        product_id = instrument_id.symbol.value.replace("/", "-")
        await self._ws_client.unsubscribe([product_id], "ticker")
        self._log.info(f"Unsubscribed from ticker for {instrument_id}")

