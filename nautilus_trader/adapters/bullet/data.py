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

from __future__ import annotations

import asyncio
from typing import Any

from nautilus_trader.adapters.bullet.config import BulletDataClientConfig
from nautilus_trader.adapters.bullet.constants import BULLET_VENUE
from nautilus_trader.adapters.bullet.providers import BulletInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import SubscribeInstrument
from nautilus_trader.data.messages import SubscribeInstruments
from nautilus_trader.data.messages import SubscribeMarkPrices
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeMarkPrices
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.identifiers import ClientId


class BulletDataClient(LiveMarketDataClient):
    """
    Provides a data client for the Bullet.xyz perpetuals exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    http_client : nautilus_pyo3.BulletHttpClient
        The Bullet HTTP client.
    ws_client : nautilus_pyo3.BulletWebSocketClient
        The Bullet WebSocket client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : BulletInstrumentProvider
        The instrument provider.
    config : BulletDataClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        http_client: nautilus_pyo3.BulletHttpClient,
        ws_client: nautilus_pyo3.BulletWebSocketClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: BulletInstrumentProvider,
        config: BulletDataClientConfig,
        name: str | None = None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or BULLET_VENUE.value),
            venue=BULLET_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )
        self._http_client = http_client
        self._ws_client = ws_client
        self._config = config

    @property
    def instrument_provider(self) -> BulletInstrumentProvider:
        return self._instrument_provider  # type: ignore[return-value]

    async def _connect(self) -> None:
        await self.instrument_provider.load_all_async()
        self._send_all_instruments_to_data_engine()

        instruments = self.instrument_provider.instruments_pyo3()
        await self._ws_client.connect(self._loop, instruments, self._handle_msg)
        self._log.info(f"Connected to WebSocket {self._ws_client.url}", LogColor.BLUE)

    async def _disconnect(self) -> None:
        if self._ws_client.is_connected():
            await self._ws_client.close()
            self._log.info("WebSocket closed")

    def _send_all_instruments_to_data_engine(self) -> None:
        for instrument in self.instrument_provider.get_all().values():
            self._handle_data(instrument)

    def _handle_msg(self, msg: Any) -> None:
        try:
            if nautilus_pyo3.is_pycapsule(msg):
                self._handle_data(capsule_to_data(msg))
            # str messages are order updates for the execution client — ignored here
        except Exception as e:
            self._log.exception("Error handling WebSocket message", e)

    # ── Subscriptions ────────────────────────────────────────────────────────

    async def _subscribe_instrument(self, command: SubscribeInstrument) -> None:
        pass

    async def _subscribe_instruments(self, command: SubscribeInstruments) -> None:
        pass

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        pyo3_id = nautilus_pyo3.InstrumentId.from_str(str(command.instrument_id))
        await self._ws_client.subscribe_quotes(pyo3_id)
        self._log.info(f"Subscribed to quote ticks for {command.instrument_id}")

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        pyo3_id = nautilus_pyo3.InstrumentId.from_str(str(command.instrument_id))
        await self._ws_client.subscribe_trades(pyo3_id)
        self._log.info(f"Subscribed to trade ticks for {command.instrument_id}")

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        pyo3_id = nautilus_pyo3.InstrumentId.from_str(str(command.instrument_id))
        await self._ws_client.subscribe_book(pyo3_id)
        self._log.info(f"Subscribed to order book deltas for {command.instrument_id}")

    async def _subscribe_mark_prices(self, command: SubscribeMarkPrices) -> None:
        pyo3_id = nautilus_pyo3.InstrumentId.from_str(str(command.instrument_id))
        await self._ws_client.subscribe_mark_prices(pyo3_id)
        self._log.info(f"Subscribed to mark prices for {command.instrument_id}")

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        pyo3_id = nautilus_pyo3.InstrumentId.from_str(str(command.instrument_id))
        await self._ws_client.unsubscribe_quotes(pyo3_id)

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        pyo3_id = nautilus_pyo3.InstrumentId.from_str(str(command.instrument_id))
        await self._ws_client.unsubscribe_trades(pyo3_id)

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        pyo3_id = nautilus_pyo3.InstrumentId.from_str(str(command.instrument_id))
        await self._ws_client.unsubscribe_book(pyo3_id)

    async def _unsubscribe_order_book(self, command: UnsubscribeOrderBook) -> None:
        pyo3_id = nautilus_pyo3.InstrumentId.from_str(str(command.instrument_id))
        await self._ws_client.unsubscribe_book(pyo3_id)

    async def _unsubscribe_mark_prices(self, command: UnsubscribeMarkPrices) -> None:
        pyo3_id = nautilus_pyo3.InstrumentId.from_str(str(command.instrument_id))
        await self._ws_client.unsubscribe_mark_prices(pyo3_id)

    async def _request_instrument(self, request: RequestInstrument) -> None:
        instrument = self.instrument_provider.find(request.instrument_id)
        if instrument is None:
            self._log.error(f"Instrument {request.instrument_id} not found")
            return
        self._handle_data(instrument)

    async def _request_instruments(self, request: RequestInstruments) -> None:
        for instrument in self.instrument_provider.get_all().values():
            self._handle_data(instrument)
