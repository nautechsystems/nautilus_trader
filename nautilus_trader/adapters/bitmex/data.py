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

from nautilus_trader.adapters.bitmex.config import BitmexDataClientConfig
from nautilus_trader.adapters.bitmex.constants import BITMEX_VENUE
from nautilus_trader.adapters.bitmex.providers import BitmexInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.identifiers import ClientId


class BitmexDataClient(LiveMarketDataClient):
    """
    Provides a data client for the BitMEX centralized crypto exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.BitmexHttpClient
        The BitMEX HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : BitmexInstrumentProvider
        The instrument provider.
    config : BitMEXDataClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.BitmexHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: BitmexInstrumentProvider,
        config: BitmexDataClientConfig,
        name: str | None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or BITMEX_VENUE.value),
            venue=BITMEX_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        # Configuration
        self._config = config
        self._base_url_ws = config.base_url_ws
        self._http_client = client
        self._ws_client: nautilus_pyo3.BitmexWebSocketClient | None = None
        self._symbol_status = config.symbol_status

    def _log_runtime_error(self, message: str) -> None:
        self._log.error(message, LogColor.RED)
        raise RuntimeError(message)

    @property
    def instrument_provider(self) -> BitmexInstrumentProvider:
        return self._instrument_provider  # type: ignore

    async def _connect(self) -> None:
        pass  # TODO: Implement

    async def _disconnect(self) -> None:
        if self._ws_client:
            await self._ws_client.close()
            self._ws_client = None

    def _create_websocket_client(self) -> nautilus_pyo3.BitmexWebSocketClient:
        """
        Create a BitMEX WebSocket client.
        """
        raise NotImplementedError("BitMEX WebSocket integration temporarily disabled")

    async def _subscribe_order_book(self, command: SubscribeOrderBook) -> None:
        # TODO: Implement
        self._log.warning("Order book subscription not yet implemented")

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        # TODO: Implement
        self._log.warning("Trade ticks subscription not yet implemented")

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        # TODO: Implement
        self._log.warning("Quote ticks subscription not yet implemented")

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        # TODO: Implement
        self._log.warning("Bars subscription not yet implemented")

    async def _unsubscribe_order_book(self, command: UnsubscribeOrderBook) -> None:
        # TODO: Implement
        self._log.warning("Order book unsubscription not yet implemented")

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        # TODO: Implement
        self._log.warning("Trade ticks unsubscription not yet implemented")

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        # TODO: Implement
        self._log.warning("Quote ticks unsubscription not yet implemented")

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        # TODO: Implement
        self._log.warning("Bars unsubscription not yet implemented")

    async def _request_instruments(self, request: RequestInstruments) -> None:
        instruments = await self._http_client.request_instruments(self._symbol_status)
        for instrument in instruments:
            self._handle_instrument(instrument)
        self._send_response(
            msg_type=type(request),
            correlation_id=request.id,
        )

    async def _request_instrument(self, request: RequestInstrument) -> None:
        instruments = await self._http_client.request_instruments(self._symbol_status)
        for instrument in instruments:
            if instrument.id == request.instrument_id:
                self._handle_instrument(instrument)
                self._send_response(
                    msg_type=type(request),
                    correlation_id=request.id,
                )
                return
        self._log.warning(f"Instrument {request.instrument_id} not found")

    async def _request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        # TODO: Implement
        self._log.warning("Quote ticks request not yet implemented")

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        # TODO: Implement
        self._log.warning("Trade ticks request not yet implemented")

    async def _request_bars(self, request: RequestBars) -> None:
        # TODO: Implement
        self._log.warning("Bars request not yet implemented")
