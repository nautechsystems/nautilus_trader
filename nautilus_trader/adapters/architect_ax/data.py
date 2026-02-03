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
"""
AX Exchange data client implementation.

This module provides a LiveMarketDataClient that interfaces with Architect's WebSocket
API for market data. The client uses Rust-based HTTP and WebSocket clients exposed via
PyO3 for performance.

"""

import asyncio

from nautilus_trader.adapters.architect_ax.config import AxDataClientConfig
from nautilus_trader.adapters.architect_ax.constants import AX_VENUE
from nautilus_trader.adapters.architect_ax.providers import AxInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.data.messages import RequestBars
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
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import book_type_to_str
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId


class AxDataClient(LiveMarketDataClient):
    """
    Provides a data client for the AX Exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.AxHttpClient
        The AX Exchange HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : AxInstrumentProvider
        The instrument provider.
    config : AxDataClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.AxHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: AxInstrumentProvider,
        config: AxDataClientConfig,
        name: str | None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or AX_VENUE.value),
            venue=AX_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        self._instrument_provider: AxInstrumentProvider = instrument_provider
        self._config = config

        self._log.info(f"{config.environment=}", LogColor.BLUE)
        self._log.info(f"{config.update_instruments_interval_mins=}", LogColor.BLUE)
        self._log.info(f"{config.http_proxy_url=}", LogColor.BLUE)

        self._http_client = client
        self._ws_client: nautilus_pyo3.AxMdWebSocketClient | None = None

        if config.base_url_ws:
            self._ws_url = config.base_url_ws
        elif config.environment == nautilus_pyo3.AxEnvironment.SANDBOX:
            self._ws_url = "wss://gateway.sandbox.architect.exchange/md/ws"
        else:
            self._ws_url = "wss://gateway.architect.exchange/md/ws"

        # Track subscribed symbols to avoid duplicate WebSocket requests
        self._subscribed_symbols: set[str] = set()

        self._update_instruments_interval_mins = config.update_instruments_interval_mins
        self._update_instruments_task: asyncio.Task | None = None

    @property
    def instrument_provider(self) -> AxInstrumentProvider:
        return self._instrument_provider

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        self._send_all_instruments_to_data_engine()

        self._ws_client = nautilus_pyo3.AxMdWebSocketClient.without_auth(
            url=self._ws_url,
            heartbeat=20,
        )

        try:
            auth_token = await self._http_client.authenticate_auto()
            self._ws_client.set_auth_token(auth_token)
            self._log.info("Authenticated with AX Exchange", LogColor.BLUE)
        except ValueError as e:
            err_str = str(e)
            if "Missing credentials" in err_str or "MissingCredentials" in err_str:
                self._log.warning("No API credentials configured, some features may be unavailable")
            else:
                raise

        for inst in self._instrument_provider.instruments_pyo3():
            self._ws_client.cache_instrument(inst)

        await self._ws_client.connect(self._handle_msg)
        self._log.info("Connected to AX Exchange market data WebSocket", LogColor.BLUE)

        if self._update_instruments_interval_mins:
            self._update_instruments_task = self.create_task(
                self._update_instruments(self._update_instruments_interval_mins),
            )

    async def _disconnect(self) -> None:
        self._http_client.cancel_all_requests()

        if self._update_instruments_task:
            self._log.debug("Canceling task 'update_instruments'")
            self._update_instruments_task.cancel()
            self._update_instruments_task = None

        # Allow time for any pending unsubscribe messages
        await asyncio.sleep(0.5)

        if self._ws_client:
            self._log.info("Disconnecting from AX Exchange market data WebSocket")
            await self._ws_client.close()
            self._ws_client = None
            self._log.info("Disconnected from AX Exchange", LogColor.BLUE)

        self._subscribed_symbols.clear()

    def _send_all_instruments_to_data_engine(self) -> None:
        for currency in self._instrument_provider.currencies().values():
            self._cache.add_currency(currency)

        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)

    async def _update_instruments(self, interval_mins: int) -> None:
        while True:
            try:
                await asyncio.sleep(interval_mins * 60)
                await self._instrument_provider.initialize(reload=True)
                self._send_all_instruments_to_data_engine()
                self._log.info(
                    f"Scheduled task 'update_instruments' to run in {interval_mins} minutes",
                    LogColor.BLUE,
                )
            except asyncio.CancelledError:
                self._log.debug("Canceled task 'update_instruments'")
                return
            except Exception as e:
                self._log.error(f"Error updating instruments: {e}")

    def _get_symbol_from_instrument_id(self, instrument_id: InstrumentId) -> str:
        return instrument_id.symbol.value

    async def _subscribe_instruments(self, command) -> None:
        if self._update_instruments_interval_mins:
            self._log.info(
                f"AX does not have an instruments channel, instrument updates handled by "
                f"polling task running every {self._update_instruments_interval_mins} minutes",
                LogColor.BLUE,
            )
        else:
            self._log.warning(
                "Instruments subscription requested but update_instruments_interval_mins not configured",
            )

    async def _subscribe_instrument(self, command) -> None:
        if self._update_instruments_interval_mins:
            self._log.info(
                f"AX does not have an instruments channel, instrument updates handled by "
                f"polling task running every {self._update_instruments_interval_mins} minutes",
                LogColor.BLUE,
            )
        else:
            self._log.warning(
                "Instrument subscription requested but update_instruments_interval_mins not configured",
            )

    async def _unsubscribe_instruments(self, command) -> None:
        pass

    async def _unsubscribe_instrument(self, command) -> None:
        pass

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        if not self._ws_client:
            self._log.warning("WebSocket not connected, cannot subscribe to order book")
            return

        symbol = self._get_symbol_from_instrument_id(command.instrument_id)
        if symbol in self._subscribed_symbols:
            return

        if command.book_type == BookType.L3_MBO:
            level = nautilus_pyo3.AxMarketDataLevel.LEVEL_3
        elif command.book_type == BookType.L2_MBP:
            level = nautilus_pyo3.AxMarketDataLevel.LEVEL_2
        else:
            self._log.warning(
                f"Book type {book_type_to_str(command.book_type)} not supported, using L2",
            )
            level = nautilus_pyo3.AxMarketDataLevel.LEVEL_2

        await self._ws_client.subscribe(symbol, level)
        self._subscribed_symbols.add(symbol)
        self._log.debug(f"Subscribed to order book for {symbol} at {level}")

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        if not self._ws_client:
            self._log.warning("WebSocket not connected, cannot subscribe to quotes")
            return

        symbol = self._get_symbol_from_instrument_id(command.instrument_id)
        if symbol in self._subscribed_symbols:
            return

        await self._ws_client.subscribe(symbol, nautilus_pyo3.AxMarketDataLevel.LEVEL_1)
        self._subscribed_symbols.add(symbol)
        self._log.debug(f"Subscribed to quotes for {symbol}")

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        if not self._ws_client:
            self._log.warning("WebSocket not connected, cannot subscribe to trades")
            return

        symbol = self._get_symbol_from_instrument_id(command.instrument_id)
        if symbol in self._subscribed_symbols:
            return

        await self._ws_client.subscribe(symbol, nautilus_pyo3.AxMarketDataLevel.LEVEL_1)
        self._subscribed_symbols.add(symbol)
        self._log.debug(f"Subscribed to trades for {symbol}")

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        # TODO: Implement when candle subscription is exposed via PyO3
        self._log.warning("Bar subscription not yet implemented for AX Exchange")

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        if not self._ws_client:
            return

        symbol = self._get_symbol_from_instrument_id(command.instrument_id)
        if symbol not in self._subscribed_symbols:
            return
        self._subscribed_symbols.discard(symbol)

        await self._ws_client.unsubscribe(symbol)
        self._log.debug(f"Unsubscribed from order book for {symbol}")

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        if not self._ws_client:
            return

        symbol = self._get_symbol_from_instrument_id(command.instrument_id)
        if symbol not in self._subscribed_symbols:
            return
        self._subscribed_symbols.discard(symbol)

        await self._ws_client.unsubscribe(symbol)
        self._log.debug(f"Unsubscribed from quotes for {symbol}")

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        if not self._ws_client:
            return

        symbol = self._get_symbol_from_instrument_id(command.instrument_id)
        if symbol not in self._subscribed_symbols:
            return
        self._subscribed_symbols.discard(symbol)

        await self._ws_client.unsubscribe(symbol)
        self._log.debug(f"Unsubscribed from trades for {symbol}")

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        # TODO: Implement when candle unsubscription is exposed via PyO3
        pass

    async def _request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        self._log.error("Cannot request historical quotes: not published by AX Exchange")

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        self._log.error("Cannot request historical trades: not yet implemented for AX Exchange")

    async def _request_bars(self, request: RequestBars) -> None:
        self._log.error("Cannot request historical bars: not yet implemented for AX Exchange")

    def _handle_msg(self, msg) -> None:
        try:
            data = capsule_to_data(msg)
            self._handle_data(data)
        except Exception as e:
            self._log.exception("Error handling websocket message", e)
