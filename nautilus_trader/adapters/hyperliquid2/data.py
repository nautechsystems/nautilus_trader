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
Live data client for Hyperliquid.
"""

from nautilus_hyperliquid2 import Hyperliquid2HttpClient
from nautilus_hyperliquid2 import Hyperliquid2WebSocketClient

from nautilus_trader.adapters.hyperliquid2.providers import Hyperliquid2InstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import DataType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId


class Hyperliquid2LiveDataClient(LiveMarketDataClient):
    """
    Live data client for the Hyperliquid exchange.

    Parameters
    ----------
    http_client : Hyperliquid2HttpClient
        The Hyperliquid HTTP client.
    ws_client : Hyperliquid2WebSocketClient
        The Hyperliquid WebSocket client.
    instrument_provider : Hyperliquid2InstrumentProvider
        The instrument provider.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.

    """

    def __init__(
        self,
        http_client: Hyperliquid2HttpClient,
        ws_client: Hyperliquid2WebSocketClient,
        instrument_provider: Hyperliquid2InstrumentProvider,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> None:
        super().__init__(
            client_id=ClientId("HYPERLIQUID"),
            venue=instrument_provider.venue,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        self._http_client = http_client
        self._ws_client = ws_client
        self._instrument_provider = instrument_provider

    async def _connect(self) -> None:
        """Connect to the Hyperliquid WebSocket."""
        self._log.info("Connecting to Hyperliquid WebSocket")

        # Load instruments
        await self._instrument_provider.load_all_async()

        # Connect WebSocket
        await self._ws_client.connect()

        self._log.info("Connected to Hyperliquid WebSocket")

    async def _disconnect(self) -> None:
        """Disconnect from the Hyperliquid WebSocket."""
        self._log.info("Disconnecting from Hyperliquid WebSocket")
        # WebSocket disconnect logic would go here
        self._log.info("Disconnected from Hyperliquid WebSocket")

    # Subscription methods (stubs for now)

    async def _subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int | None = None,
        **kwargs,
    ) -> None:
        """Subscribe to order book deltas."""
        self._log.warning(
            f"Order book delta subscriptions not yet implemented for {instrument_id}"
        )

    async def _subscribe_order_book_snapshots(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int | None = None,
        **kwargs,
    ) -> None:
        """Subscribe to order book snapshots."""
        self._log.warning(
            f"Order book snapshot subscriptions not yet implemented for {instrument_id}"
        )

    async def _subscribe_ticker(self, instrument_id: InstrumentId, **kwargs) -> None:
        """Subscribe to ticker updates."""
        self._log.warning(
            f"Ticker subscriptions not yet implemented for {instrument_id}"
        )

    async def _subscribe_quote_ticks(
        self,
        instrument_id: InstrumentId,
        **kwargs,
    ) -> None:
        """Subscribe to quote ticks."""
        self._log.warning(
            f"Quote tick subscriptions not yet implemented for {instrument_id}"
        )

    async def _subscribe_trade_ticks(
        self,
        instrument_id: InstrumentId,
        **kwargs,
    ) -> None:
        """Subscribe to trade ticks."""
        self._log.warning(
            f"Trade tick subscriptions not yet implemented for {instrument_id}"
        )

    async def _subscribe_bars(self, bar_type: BarType, **kwargs) -> None:
        """Subscribe to bars."""
        self._log.warning(f"Bar subscriptions not yet implemented for {bar_type}")

    async def _subscribe_instrument_status(
        self,
        instrument_id: InstrumentId,
        **kwargs,
    ) -> None:
        """Subscribe to instrument status updates."""
        self._log.warning(
            f"Instrument status subscriptions not yet implemented for {instrument_id}"
        )

    async def _subscribe_instrument_close(
        self,
        instrument_id: InstrumentId,
        **kwargs,
    ) -> None:
        """Subscribe to instrument close prices."""
        self._log.warning(
            f"Instrument close subscriptions not yet implemented for {instrument_id}"
        )

    async def _subscribe_instrument(
        self,
        instrument_id: InstrumentId,
        **kwargs,
    ) -> None:
        """Subscribe to instrument updates."""
        self._log.warning(
            f"Instrument subscriptions not yet implemented for {instrument_id}"
        )

    async def _subscribe_instruments(self, **kwargs) -> None:
        """Subscribe to all instrument updates."""
        self._log.warning("Instruments subscriptions not yet implemented")

    # Unsubscribe methods (stubs for now)

    async def _unsubscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        **kwargs,
    ) -> None:
        """Unsubscribe from order book deltas."""
        pass

    async def _unsubscribe_order_book_snapshots(
        self,
        instrument_id: InstrumentId,
        **kwargs,
    ) -> None:
        """Unsubscribe from order book snapshots."""
        pass

    async def _unsubscribe_ticker(self, instrument_id: InstrumentId, **kwargs) -> None:
        """Unsubscribe from ticker updates."""
        pass

    async def _unsubscribe_quote_ticks(
        self,
        instrument_id: InstrumentId,
        **kwargs,
    ) -> None:
        """Unsubscribe from quote ticks."""
        pass

    async def _unsubscribe_trade_ticks(
        self,
        instrument_id: InstrumentId,
        **kwargs,
    ) -> None:
        """Unsubscribe from trade ticks."""
        pass

    async def _unsubscribe_bars(self, bar_type: BarType, **kwargs) -> None:
        """Unsubscribe from bars."""
        pass

    async def _unsubscribe_instrument_status(
        self,
        instrument_id: InstrumentId,
        **kwargs,
    ) -> None:
        """Unsubscribe from instrument status updates."""
        pass

    async def _unsubscribe_instrument_close(
        self,
        instrument_id: InstrumentId,
        **kwargs,
    ) -> None:
        """Unsubscribe from instrument close prices."""
        pass

    async def _unsubscribe_instrument(
        self,
        instrument_id: InstrumentId,
        **kwargs,
    ) -> None:
        """Unsubscribe from instrument updates."""
        pass

    async def _unsubscribe_instruments(self, **kwargs) -> None:
        """Unsubscribe from all instrument updates."""
        pass

    # Request methods (stubs for now)

    async def _request_data(self, data_type: DataType, **kwargs) -> None:
        """Request data."""
        self._log.warning(f"Data requests not yet implemented for {data_type}")

    async def _request_quote_ticks(
        self,
        instrument_id: InstrumentId,
        **kwargs,
    ) -> None:
        """Request historical quote ticks."""
        self._log.warning(
            f"Quote tick requests not yet implemented for {instrument_id}"
        )

    async def _request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        **kwargs,
    ) -> None:
        """Request historical trade ticks."""
        self._log.warning(
            f"Trade tick requests not yet implemented for {instrument_id}"
        )

    async def _request_bars(self, bar_type: BarType, **kwargs) -> None:
        """Request historical bars."""
        self._log.warning(f"Bar requests not yet implemented for {bar_type}")

    async def _request_instrument(
        self,
        instrument_id: InstrumentId,
        **kwargs,
    ) -> None:
        """Request instrument."""
        self._log.warning(f"Instrument requests not yet implemented for {instrument_id}")

    async def _request_instruments(self, **kwargs) -> None:
        """Request instruments."""
        self._log.warning("Instruments requests not yet implemented")
