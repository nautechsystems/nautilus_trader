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
from typing import Any

from nautilus_trader.adapters.hyperliquid.config import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid.constants import HYPERLIQUID_VENUE
from nautilus_trader.adapters.hyperliquid.providers import HyperliquidInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.datetime import ensure_pydatetime_utc
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeFundingRates
from nautilus_trader.data.messages import SubscribeIndexPrices
from nautilus_trader.data.messages import SubscribeInstrument
from nautilus_trader.data.messages import SubscribeInstruments
from nautilus_trader.data.messages import SubscribeMarkPrices
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeFundingRates
from nautilus_trader.data.messages import UnsubscribeIndexPrices
from nautilus_trader.data.messages import UnsubscribeInstrument
from nautilus_trader.data.messages import UnsubscribeInstruments
from nautilus_trader.data.messages import UnsubscribeMarkPrices
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.cancellation import DEFAULT_FUTURE_CANCELLATION_TIMEOUT
from nautilus_trader.live.cancellation import cancel_tasks_with_timeout
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import FundingRateUpdate
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.identifiers import ClientId


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
        self._log.info(f"{config.http_proxy_url=}", LogColor.BLUE)
        self._log.info(f"{config.ws_proxy_url=}", LogColor.BLUE)

        # HTTP client
        self._http_client = client
        # TODO: HyperliquidHttpClient doesn't expose api_key attribute yet
        self._log.info("HTTP client initialized", LogColor.BLUE)

        # WebSocket clients
        self._ws_clients: dict[
            nautilus_pyo3.HyperliquidProductType,
            nautilus_pyo3.HyperliquidWebSocketClient,
        ] = {}
        self._ws_client_futures: set[asyncio.Future] = set()

        for product_type_str in ["PERP", "SPOT"]:
            product_type = nautilus_pyo3.HyperliquidProductType.from_str(product_type_str)
            ws_client = nautilus_pyo3.HyperliquidWebSocketClient(
                url=config.base_url_ws,
                testnet=config.testnet,
                product_type=product_type,
            )
            self._ws_clients[product_type] = ws_client
            self._log.info(f"Initialized WebSocket client for {product_type_str}", LogColor.BLUE)

    @property
    def instrument_provider(self) -> HyperliquidInstrumentProvider:
        return self._instrument_provider

    async def _connect(self) -> None:
        await self.instrument_provider.initialize()
        self._cache_instruments()
        self._send_all_instruments_to_data_engine()

        instruments = self.instrument_provider.instruments_pyo3()

        # Connect all WebSocket clients
        for product_type_str, ws_client in self._ws_clients.items():
            await ws_client.connect(
                instruments,
                self._handle_msg,
            )
            # NOTE: wait_until_active is not yet implemented in the Hyperliquid WebSocket client
            # The connection still works without it, but we lose the synchronization guarantee
            # that the WebSocket is fully active before subscribing
            # TODO: Implement wait_until_active in HyperliquidWebSocketClient (Rust side)
            # await ws_client.wait_until_active(timeout_secs=10.0)
            self._log.info(
                f"Connected to {product_type_str} WebSocket {ws_client.url}",
                LogColor.BLUE,
            )

    async def _disconnect(self) -> None:
        # Note: PyO3 HyperliquidHttpClient doesn't expose cancel_all_requests method
        # The client will be cleaned up automatically when the object is destroyed

        # Delay to allow websocket to send any unsubscribe messages
        await asyncio.sleep(1.0)

        # Shutdown all WebSocket clients
        for product_type_str, ws_client in self._ws_clients.items():
            if not ws_client.is_closed():
                self._log.info(f"Disconnecting {product_type_str} WebSocket")
                await ws_client.close()
                self._log.info(
                    f"Disconnected from {product_type_str} WebSocket {ws_client.url}",
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
        # Ensures instrument definitions are available for correct
        # price and size precisions when parsing responses
        instruments_pyo3 = self.instrument_provider.instruments_pyo3()
        for inst in instruments_pyo3:
            self._http_client.cache_instrument(inst)

        self._log.debug("Cached instruments", LogColor.MAGENTA)

    def _send_all_instruments_to_data_engine(self) -> None:
        for instrument in self.instrument_provider.get_all().values():
            self._handle_data(instrument)

        for currency in self.instrument_provider.currencies().values():
            self._cache.add_currency(currency)

    def _handle_msg(self, msg: Any) -> None:
        try:
            if nautilus_pyo3.is_pycapsule(msg):
                # The capsule will fall out of scope at the end of this method,
                # and eventually be garbage collected. The contained pointer
                # to `Data` is still owned and managed by Rust.
                data = capsule_to_data(msg)
                self._handle_data(data)
            elif isinstance(msg, nautilus_pyo3.FundingRateUpdate):
                data = FundingRateUpdate.from_pyo3(msg)
                self._handle_data(data)
            else:
                self._log.warning(f"Cannot handle message {msg}: not implemented")
        except Exception as e:
            self._log.exception("Error handling websocket message", e)

    def _get_ws_client_for_instrument(
        self,
        instrument_id: nautilus_pyo3.InstrumentId,
    ) -> nautilus_pyo3.HyperliquidWebSocketClient:
        product_type = nautilus_pyo3.hyperliquid_product_type_from_symbol(
            instrument_id.symbol.value,
        )
        ws_client = self._ws_clients.get(product_type)
        if ws_client is None:
            raise ValueError(
                f"No WebSocket client configured for product type {product_type}",
            )
        return ws_client

    # -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    async def _subscribe_instrument(self, command: SubscribeInstrument) -> None:
        self._log.info(f"Subscribed to instrument updates for {command.instrument_id}")

    async def _subscribe_instruments(self, command: SubscribeInstruments) -> None:
        self._log.info("Subscribed to instruments updates")

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.subscribe_book(pyo3_instrument_id)

    async def _subscribe_order_book_snapshots(self, command: SubscribeOrderBook) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.subscribe_book(pyo3_instrument_id)

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.subscribe_quotes(pyo3_instrument_id)

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.subscribe_trades(pyo3_instrument_id)

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(command.bar_type))
        pyo3_instrument_id = pyo3_bar_type.instrument_id
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.subscribe_bars(pyo3_bar_type)

    async def _subscribe_mark_prices(self, command: SubscribeMarkPrices) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.subscribe_mark_prices(pyo3_instrument_id)

    async def _subscribe_index_prices(self, command: SubscribeIndexPrices) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.subscribe_index_prices(pyo3_instrument_id)

    async def _subscribe_funding_rates(self, command: SubscribeFundingRates) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.subscribe_funding_rates(pyo3_instrument_id)

    async def _unsubscribe_instrument(self, command: UnsubscribeInstrument) -> None:
        self._log.info(f"Unsubscribed from instrument updates for {command.instrument_id}")

    async def _unsubscribe_instruments(self, command: UnsubscribeInstruments) -> None:
        self._log.info("Unsubscribed from instruments updates")

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.unsubscribe_book(pyo3_instrument_id)

    async def _unsubscribe_order_book_snapshots(self, command: UnsubscribeOrderBook) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.unsubscribe_book(pyo3_instrument_id)

    async def _unsubscribe_order_book(self, command: UnsubscribeOrderBook) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.unsubscribe_book(pyo3_instrument_id)

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.unsubscribe_quotes(pyo3_instrument_id)

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.unsubscribe_trades(pyo3_instrument_id)

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(command.bar_type))
        pyo3_instrument_id = pyo3_bar_type.instrument_id
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.unsubscribe_bars(pyo3_bar_type)

    async def _unsubscribe_mark_prices(self, command: UnsubscribeMarkPrices) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.unsubscribe_mark_prices(pyo3_instrument_id)

    async def _unsubscribe_index_prices(self, command: UnsubscribeIndexPrices) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.unsubscribe_index_prices(pyo3_instrument_id)

    async def _unsubscribe_funding_rates(self, command: UnsubscribeFundingRates) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.unsubscribe_funding_rates(pyo3_instrument_id)

    # -- REQUESTS -----------------------------------------------------------------------------------

    async def _request_instrument(self, request: RequestInstrument) -> None:
        instrument = self.instrument_provider.find(request.instrument_id)
        if instrument:
            self._handle_data(instrument)
            self._log.debug(f"Sent instrument {request.instrument_id}")
        else:
            self._log.error(f"Instrument not found: {request.instrument_id}")

    async def _request_instruments(self, request: RequestInstruments) -> None:
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

    async def _request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        self._log.warning("Cannot request historical quotes: not supported by Hyperliquid")

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        self._log.warning("Cannot request historical trades: not supported by Hyperliquid")

    async def _request_bars(self, request: RequestBars) -> None:
        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(request.bar_type))
        start = ensure_pydatetime_utc(request.start) if request.start else None
        end = ensure_pydatetime_utc(request.end) if request.end else None

        try:
            pyo3_bars = await self._http_client.request_bars(
                pyo3_bar_type,
                start,
                end,
                request.limit,
            )
            bars = Bar.from_pyo3_list(pyo3_bars)

            self._handle_bars(
                request.bar_type,
                bars,
                request.id,
                request.start,
                request.end,
                request.params,
            )
        except Exception as e:
            self._log.exception(f"Error requesting bars for {request.bar_type}", e)
