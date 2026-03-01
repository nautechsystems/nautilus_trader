# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software distributed under the
#  License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
#  KIND, either express or implied. See the License for the specific language governing
#  permissions and limitations under the License.
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import asyncio
from typing import TYPE_CHECKING

from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.identifiers import ClientId

from nautilus_trader.adapters.kalshi.config import KalshiDataClientConfig
from nautilus_trader.adapters.kalshi.providers import KalshiInstrumentProvider

if TYPE_CHECKING:
    from nautilus_trader.cache.cache import Cache
    from nautilus_trader.common.component import LiveClock
    from nautilus_trader.common.component import MessageBus

KALSHI_CLIENT_ID = "KALSHI"


class KalshiDataClient(LiveMarketDataClient):
    """
    Provides a Kalshi market data client for live paper trading and backtesting.

    For backtesting, this client feeds historical REST data into the engine.
    For live paper trading, it uses authenticated WebSocket streams.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop.
    client_id : ClientId
        The data client ID.
    msgbus : MessageBus
        The message bus.
    cache : Cache
        The cache.
    clock : LiveClock
        The clock.
    instrument_provider : KalshiInstrumentProvider
        The instrument provider.
    config : KalshiDataClientConfig
        The adapter configuration.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: KalshiInstrumentProvider,
        config: KalshiDataClientConfig,
        name: str | None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or KALSHI_CLIENT_ID),
            venue=None,  # Multi-venue adapter
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )
        self._config = config
        self._instrument_provider = instrument_provider

    async def _connect(self) -> None:
        await self._instrument_provider.load_all_async()

    async def _disconnect(self) -> None:
        pass

    # TODO: implement subscribe_order_book_deltas, subscribe_trade_ticks,
    # subscribe_bars using the Rust WebSocket client and HTTP candlestick client.
    # Reference: nautilus_trader/adapters/polymarket/data.py for subscription patterns.
