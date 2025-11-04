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
Data client for Asterdex adapters.
"""

import asyncio
from collections.abc import Callable

import pandas as pd

from nautilus_trader.adapters.asterdex2.config import AsterdexDataClientConfig
from nautilus_trader.adapters.asterdex2.http.client import AsterdexHttpClient
from nautilus_trader.adapters.asterdex2.providers import AsterdexInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.live.data_client import LiveDataClient
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue


class AsterdexLiveDataClient(LiveDataClient):
    """
    Provides a data client for the Asterdex exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    http_client : AsterdexHttpClient
        The Asterdex HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : AsterdexInstrumentProvider
        The instrument provider for the client.
    config : AsterdexDataClientConfig
        The configuration for the client.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        http_client: AsterdexHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: AsterdexInstrumentProvider,
        config: AsterdexDataClientConfig,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(config.name or "ASTERDEX"),
            venue=Venue("ASTERDEX"),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        # Configuration
        self._config = config
        self._http_client = http_client

        # WebSocket client (to be implemented)
        self._ws_client = None

        self._log.info("Asterdex data client initialized", LogColor.BLUE)

    async def _connect(self) -> None:
        """Connect the client."""
        self._log.info("Connecting to Asterdex...", LogColor.BLUE)

        # Load instruments
        await self._instrument_provider.load_all_async()

        self._log.info("Connected to Asterdex", LogColor.GREEN)

    async def _disconnect(self) -> None:
        """Disconnect the client."""
        self._log.info("Disconnecting from Asterdex...", LogColor.BLUE)

        # Disconnect WebSocket if connected
        if self._ws_client:
            # TODO: Implement WebSocket disconnection
            pass

        self._log.info("Disconnected from Asterdex", LogColor.GREEN)

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    async def _subscribe_instruments(self) -> None:
        """Subscribe to all instrument updates."""
        # Not implemented for Asterdex
        pass

    async def _subscribe_instrument(self, instrument_id: InstrumentId) -> None:
        """Subscribe to a specific instrument."""
        # Not implemented for Asterdex
        pass

    async def _subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int | None = None,
        kwargs: dict | None = None,
    ) -> None:
        """Subscribe to order book deltas."""
        # TODO: Implement WebSocket subscription
        self._log.warning("Order book delta subscriptions not yet implemented")

    async def _subscribe_order_book_snapshots(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int | None = None,
        kwargs: dict | None = None,
    ) -> None:
        """Subscribe to order book snapshots."""
        # TODO: Implement WebSocket subscription
        self._log.warning("Order book snapshot subscriptions not yet implemented")

    async def _subscribe_ticker(self, instrument_id: InstrumentId) -> None:
        """Subscribe to ticker updates."""
        # TODO: Implement WebSocket subscription
        self._log.warning("Ticker subscriptions not yet implemented")

    async def _subscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        """Subscribe to quote ticks."""
        # TODO: Implement WebSocket subscription
        self._log.warning("Quote tick subscriptions not yet implemented")

    async def _subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        """Subscribe to trade ticks."""
        # TODO: Implement WebSocket subscription
        self._log.warning("Trade tick subscriptions not yet implemented")

    async def _subscribe_bars(self, bar_type: BarType) -> None:
        """Subscribe to bars."""
        # TODO: Implement WebSocket subscription
        self._log.warning("Bar subscriptions not yet implemented")

    # -- UNSUBSCRIPTIONS --------------------------------------------------------------------------

    async def _unsubscribe_instruments(self) -> None:
        """Unsubscribe from all instrument updates."""
        pass

    async def _unsubscribe_instrument(self, instrument_id: InstrumentId) -> None:
        """Unsubscribe from a specific instrument."""
        pass

    async def _unsubscribe_order_book_deltas(self, instrument_id: InstrumentId) -> None:
        """Unsubscribe from order book deltas."""
        pass

    async def _unsubscribe_order_book_snapshots(self, instrument_id: InstrumentId) -> None:
        """Unsubscribe from order book snapshots."""
        pass

    async def _unsubscribe_ticker(self, instrument_id: InstrumentId) -> None:
        """Unsubscribe from ticker updates."""
        pass

    async def _unsubscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        """Unsubscribe from quote ticks."""
        pass

    async def _unsubscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        """Unsubscribe from trade ticks."""
        pass

    async def _unsubscribe_bars(self, bar_type: BarType) -> None:
        """Unsubscribe from bars."""
        pass

    # -- REQUESTS ---------------------------------------------------------------------------------

    async def _request_instrument(
        self,
        instrument_id: InstrumentId,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        """Request a specific instrument."""
        # Instrument should already be loaded
        instrument = self._cache.instrument(instrument_id)
        if instrument:
            self._handle_data_response(
                data_type=type(instrument),
                data=instrument,
                correlation_id=correlation_id,
            )

    async def _request_instruments(
        self,
        venue: Venue,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        """Request all instruments for a venue."""
        # Instruments should already be loaded
        instruments = self._cache.instruments(venue)
        if instruments:
            self._handle_data_response(
                data_type=type(instruments[0]),
                data=instruments,
                correlation_id=correlation_id,
            )

    async def _request_quote_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        """Request historical quote ticks."""
        # Not implemented
        self._log.warning("Historical quote ticks not implemented")

    async def _request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        """Request historical trade ticks."""
        # Not implemented
        self._log.warning("Historical trade ticks not implemented")

    async def _request_bars(
        self,
        bar_type: BarType,
        limit: int,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        """Request historical bars."""
        # Not implemented
        self._log.warning("Historical bars not implemented")
