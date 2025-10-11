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

from nautilus_trader.adapters.hyperliquid.config import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid.constants import HYPERLIQUID_VENUE
from nautilus_trader.adapters.hyperliquid.providers import HyperliquidInstrumentProvider
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
from nautilus_trader.data.messages import SubscribeInstrument
from nautilus_trader.data.messages import SubscribeInstruments
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeInstrument
from nautilus_trader.data.messages import UnsubscribeInstruments
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.cancellation import DEFAULT_FUTURE_CANCELLATION_TIMEOUT
from nautilus_trader.live.cancellation import cancel_tasks_with_timeout
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.identifiers import ClientId


if TYPE_CHECKING:
    from typing import Any


# -------------------------------------------------------------------------------------------------
# Helper Functions
# -------------------------------------------------------------------------------------------------


def _get_hyperliquid_ws_url(is_testnet: bool = False) -> str:
    """
    Get the appropriate Hyperliquid WebSocket URL.
    """
    if is_testnet:
        return "wss://api.hyperliquid-testnet.xyz/ws"
    return "wss://api.hyperliquid.xyz/ws"


# -------------------------------------------------------------------------------------------------
# Data Client
# -------------------------------------------------------------------------------------------------


class HyperliquidDataClient(LiveMarketDataClient):
    """
    Provides a data client for the Hyperliquid decentralized exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.HyperliquidHttpClient
        The Hyperliquid HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : HyperliquidInstrumentProvider
        The instrument provider.
    config : HyperliquidDataClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: Any,  # nautilus_pyo3.HyperliquidHttpClient
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: HyperliquidInstrumentProvider,
        config: HyperliquidDataClientConfig,
        name: str | None = None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or HYPERLIQUID_VENUE.value),
            venue=HYPERLIQUID_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        self._instrument_provider: HyperliquidInstrumentProvider = instrument_provider

        # Configuration
        self._config = config
        self._log.info(f"config.testnet={config.testnet}", LogColor.BLUE)
        self._log.info(f"config.http_timeout_secs={config.http_timeout_secs}", LogColor.BLUE)

        # HTTP client
        self._http_client = client
        # Note: PyO3 HyperliquidHttpClient doesn't expose api_key attribute
        self._log.info("HTTP client initialized", LogColor.BLUE)

        # WebSocket client
        ws_url = self._determine_ws_url(config)
        self._log.info(f"WebSocket URL: {ws_url}", LogColor.BLUE)

        # Initialize HyperliquidWebSocketClient from nautilus_pyo3
        self._ws_client = nautilus_pyo3.HyperliquidWebSocketClient(url=ws_url)  # type: ignore[attr-defined]
        self._ws_client_futures: set[asyncio.Future] = set()

    def _determine_ws_url(self, config: HyperliquidDataClientConfig) -> str:
        """
        Determine the WebSocket URL based on configuration.
        """
        if config.base_url_ws:
            return config.base_url_ws
        return _get_hyperliquid_ws_url(config.testnet)

    @property
    def instrument_provider(self) -> HyperliquidInstrumentProvider:
        return self._instrument_provider

    async def _connect(self) -> None:
        """
        Connect the client following standard patterns.
        """
        await self.instrument_provider.initialize()
        self._cache_instruments()
        self._send_all_instruments_to_data_engine()

        # Connect WebSocket following BitMEX pattern with positional arguments
        instruments = self.instrument_provider.instruments_pyo3()
        await self._ws_client.connect(
            instruments,
            self._handle_msg,
        )
        # NOTE: wait_until_active is not yet implemented in the Hyperliquid WebSocket client
        # The connection still works without it, but we lose the synchronization guarantee
        # that the WebSocket is fully active before subscribing
        # TODO: Implement wait_until_active in HyperliquidWebSocketClient (Rust side)
        # await self._ws_client.wait_until_active(timeout_secs=10.0)
        self._log.info(f"Connected to WebSocket {self._ws_client.url}", LogColor.BLUE)

    async def _disconnect(self) -> None:
        """
        Disconnect the client following standard patterns.
        """
        # Note: PyO3 HyperliquidHttpClient doesn't expose cancel_all_requests method
        # The client will be cleaned up automatically when the object is destroyed

        # Delay to allow websocket to send any unsubscribe messages
        await asyncio.sleep(1.0)

        # Shutdown WebSocket following OKX/BitMEX pattern
        if not self._ws_client.is_closed():
            self._log.info("Disconnecting WebSocket")
            await self._ws_client.close()
            self._log.info(
                f"Disconnected from {self._ws_client.url}",
                LogColor.BLUE,
            )

        # Cancel all WebSocket client futures with timeout
        if self._ws_client_futures:
            self._log.debug(f"Canceling {len(self._ws_client_futures)} WebSocket client futures...")
            await cancel_tasks_with_timeout(
                self._ws_client_futures,
                timeout_secs=DEFAULT_FUTURE_CANCELLATION_TIMEOUT,
                logger=self._log,
            )
            self._ws_client_futures.clear()

        self._log.info("Disconnected from Hyperliquid", LogColor.GREEN)

    def _cache_instruments(self) -> None:
        """
        Cache instruments following OKX/BitMEX pattern.
        """
        # Ensures instrument definitions are available for correct
        # price and size precisions when parsing responses
        instruments_pyo3 = self.instrument_provider.instruments_pyo3()
        for inst in instruments_pyo3:
            self._http_client.add_instrument(inst)

        self._log.debug("Cached instruments", LogColor.MAGENTA)

    def _send_all_instruments_to_data_engine(self) -> None:
        """
        Send all instruments to data engine.

        Follows the same pattern as other PyO3-based venues (BitMEX, OKX). Uses
        _handle_data() which properly routes instruments through the data engine.

        """
        for instrument in self.instrument_provider.get_all().values():
            self._handle_data(instrument)

        for currency in self.instrument_provider.currencies().values():
            self._cache.add_currency(currency)

    def _handle_msg(self, message: bytes) -> None:
        """
        Handle WebSocket messages following nautilus_pyo3 callback pattern.

        This method receives raw bytes from the WebSocket and processes them into
        appropriate Nautilus data types using the capsule_to_data pattern.

        """
        try:
            data_obj = capsule_to_data(message)
            self._handle_data(data_obj)
        except Exception as e:
            self._log.error(f"Error handling message: {e}")

    # -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        """
        Subscribe to trade ticks following standard pattern.
        """
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_trades(pyo3_instrument_id)

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        """
        Subscribe to quote ticks following standard pattern.
        """
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_quotes(pyo3_instrument_id)

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        """
        Subscribe to order book deltas following standard pattern.
        """
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_order_book_deltas(
            pyo3_instrument_id,
            command.book_type,
            command.depth if command.depth else 0,
        )

    async def _subscribe_order_book_snapshots(self, command: SubscribeOrderBook) -> None:
        """
        Subscribe to order book snapshots following standard pattern.
        """
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_order_book_snapshots(
            pyo3_instrument_id,
            command.book_type,
            command.depth if command.depth else 0,
        )

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        """
        Subscribe to bars following standard pattern.
        """
        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(command.bar_type))
        await self._ws_client.subscribe_bars(pyo3_bar_type)

    async def _subscribe_instrument(self, command: SubscribeInstrument) -> None:
        """
        Subscribe to instrument updates.
        """
        self._log.info(f"Subscribed to instrument updates for {command.instrument_id}")

    async def _subscribe_instruments(self, command: SubscribeInstruments) -> None:
        """
        Subscribe to instruments updates.
        """
        self._log.info("Subscribed to instruments updates")

    # -- UNSUBSCRIPTIONS ---------------------------------------------------------------------------

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        """
        Unsubscribe from trade ticks.
        """
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_trades(pyo3_instrument_id)

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        """
        Unsubscribe from quote ticks.
        """
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_quotes(pyo3_instrument_id)

    async def _unsubscribe_order_book(self, command: UnsubscribeOrderBook) -> None:
        """
        Unsubscribe from order book updates.
        """
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_order_book_deltas(pyo3_instrument_id)

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        """
        Unsubscribe from bars.
        """
        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(command.bar_type))
        await self._ws_client.unsubscribe_bars(pyo3_bar_type)

    async def _unsubscribe_instrument(self, command: UnsubscribeInstrument) -> None:
        """
        Unsubscribe from instrument updates.
        """
        self._log.info(f"Unsubscribed from instrument updates for {command.instrument_id}")

    async def _unsubscribe_instruments(self, command: UnsubscribeInstruments) -> None:
        """
        Unsubscribe from instruments updates.
        """
        self._log.info("Unsubscribed from instruments updates")

    # -- REQUESTS -----------------------------------------------------------------------------------

    async def _request_instrument(self, request: RequestInstrument) -> None:
        """
        Request instrument definition following standard pattern.
        """
        instrument = self.instrument_provider.find(request.instrument_id)
        if instrument:
            self._handle_data(instrument)
            self._log.debug(f"Sent instrument {request.instrument_id}")
        else:
            self._log.error(f"Instrument not found: {request.instrument_id}")

    async def _request_instruments(self, request: RequestInstruments) -> None:
        """
        Request multiple instrument definitions following standard pattern.
        """
        instruments = []
        for instrument_id in request.instrument_ids:
            instrument = self.instrument_provider.find(instrument_id)
            if instrument:
                instruments.append(instrument)
                self._handle_data(instrument)
                self._log.debug(f"Sent instrument {instrument_id}")
            else:
                self._log.warning(f"Instrument not found: {instrument_id}")

        if not instruments:
            self._log.warning("No instruments found for request")
        else:
            self._log.info(f"Sent {len(instruments)} instruments")

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        """
        Request historical trade ticks.
        """
        try:
            pyo3_trades = await self._http_client.request_trade_ticks(
                request.instrument_id,
                request.start,
                request.end,
                request.limit,
            )
            trade_ticks = TradeTick.from_pyo3_list(pyo3_trades)
            for trade_tick in trade_ticks:
                self._handle_data(trade_tick)
        except Exception as e:
            self._log.error(f"Error requesting trade ticks for {request.instrument_id}: {e}")

    async def _request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        """
        Request historical quote ticks.
        """
        try:
            pyo3_quotes = await self._http_client.request_quote_ticks(
                request.instrument_id,
                request.start,
                request.end,
                request.limit,
            )
            quote_ticks = QuoteTick.from_pyo3_list(pyo3_quotes)
            for quote_tick in quote_ticks:
                self._handle_data(quote_tick)
        except Exception as e:
            self._log.error(f"Error requesting quote ticks for {request.instrument_id}: {e}")

    async def _request_bars(self, request: RequestBars) -> None:
        """
        Request historical bars.
        """
        try:
            pyo3_bars = await self._http_client.request_bars(
                request.bar_type,
                request.start,
                request.end,
                request.limit,
            )
            bars = Bar.from_pyo3_list(pyo3_bars)
            for bar in bars:
                self._handle_data(bar)
        except Exception as e:
            self._log.error(f"Error requesting bars for {request.bar_type}: {e}")
