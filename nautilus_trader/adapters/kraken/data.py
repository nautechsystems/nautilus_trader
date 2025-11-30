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
from typing import Any

from nautilus_trader.adapters.kraken.config import KrakenDataClientConfig
from nautilus_trader.adapters.kraken.constants import KRAKEN_VENUE
from nautilus_trader.adapters.kraken.providers import KrakenInstrumentProvider
from nautilus_trader.adapters.kraken.types import KRAKEN_INSTRUMENT_TYPES
from nautilus_trader.adapters.kraken.types import KrakenInstrument
from nautilus_trader.cache.cache import Cache
from nautilus_trader.cache.transformers import transform_instrument_from_pyo3
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import KrakenEnvironment
from nautilus_trader.core.nautilus_pyo3 import KrakenProductType
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeIndexPrices
from nautilus_trader.data.messages import SubscribeInstrument
from nautilus_trader.data.messages import SubscribeInstruments
from nautilus_trader.data.messages import SubscribeMarkPrices
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
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
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import book_type_to_str
from nautilus_trader.model.identifiers import ClientId


class KrakenDataClient(LiveMarketDataClient):
    """
    Provides a data client for the Kraken centralized crypto exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    http_client_spot : nautilus_pyo3.KrakenSpotHttpClient, optional
        The Kraken Spot HTTP client.
    http_client_futures : nautilus_pyo3.KrakenFuturesHttpClient, optional
        The Kraken Futures HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : KrakenInstrumentProvider
        The instrument provider.
    config : KrakenDataClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        http_client_spot: nautilus_pyo3.KrakenSpotHttpClient | None,
        http_client_futures: nautilus_pyo3.KrakenFuturesHttpClient | None,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: KrakenInstrumentProvider,
        config: KrakenDataClientConfig,
        name: str | None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or KRAKEN_VENUE.value),
            venue=KRAKEN_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        # Configuration
        self._config = config
        self._product_types = list(config.product_types or [KrakenProductType.SPOT])

        self._log.info(f"product_types={self._product_types}", LogColor.BLUE)
        self._log.info(f"{config.base_url_http_spot=}", LogColor.BLUE)
        self._log.info(f"{config.base_url_http_futures=}", LogColor.BLUE)
        self._log.info(f"{config.base_url_ws=}", LogColor.BLUE)
        self._log.info(f"{config.update_instruments_interval_mins=}", LogColor.BLUE)
        self._log.info(f"{config.ws_heartbeat_secs=}", LogColor.BLUE)

        # HTTP API clients
        self._http_client_spot = http_client_spot
        self._http_client_futures = http_client_futures

        # Log API keys for configured clients
        if http_client_spot is not None:
            masked_key = http_client_spot.api_key_masked
            self._log.info(f"SPOT REST API key {masked_key}", LogColor.BLUE)
        if http_client_futures is not None:
            masked_key = http_client_futures.api_key_masked
            self._log.info(f"FUTURES REST API key {masked_key}", LogColor.BLUE)

        # Determine environment
        environment = config.environment or KrakenEnvironment.MAINNET

        # WebSocket API - Spot (Kraken v2 API)
        self._ws_client_spot: nautilus_pyo3.KrakenSpotWebSocketClient | None = None
        self._ws_client_spot_connected = False
        if KrakenProductType.SPOT in self._product_types:
            self._ws_client_spot = nautilus_pyo3.KrakenSpotWebSocketClient(
                environment=environment,
                base_url=config.base_url_ws,
                heartbeat_secs=config.ws_heartbeat_secs,
            )
            self._log.info(f"Spot WebSocket URL {self._ws_client_spot.url}", LogColor.BLUE)

        # WebSocket API - Futures (Kraken v1 API)
        self._ws_client_futures: nautilus_pyo3.KrakenFuturesWebSocketClient | None = None
        self._ws_client_futures_connected = False
        if KrakenProductType.FUTURES in self._product_types:
            self._ws_client_futures = nautilus_pyo3.KrakenFuturesWebSocketClient(
                environment=environment,
                heartbeat_secs=config.ws_heartbeat_secs,
            )
            self._log.info(f"Futures WebSocket URL {self._ws_client_futures.url}", LogColor.BLUE)

        self._ws_client_async_futures: set[asyncio.Future] = set()

        self._update_instruments_task: asyncio.Task | None = None

    @property
    def instrument_provider(self) -> KrakenInstrumentProvider:
        return self._instrument_provider  # type: ignore

    def _get_http_client_for_symbol(
        self,
        symbol: str,
    ) -> nautilus_pyo3.KrakenSpotHttpClient | nautilus_pyo3.KrakenFuturesHttpClient | None:
        product_type = nautilus_pyo3.kraken_product_type_from_symbol(symbol)
        if product_type == KrakenProductType.SPOT:
            return self._http_client_spot
        elif product_type == KrakenProductType.FUTURES:
            return self._http_client_futures
        return None

    def _get_ws_client_for_symbol(
        self,
        symbol: str,
    ) -> (
        nautilus_pyo3.KrakenSpotWebSocketClient
        | nautilus_pyo3.KrakenFuturesWebSocketClient
        | None
    ):
        product_type = nautilus_pyo3.kraken_product_type_from_symbol(symbol)
        if product_type == KrakenProductType.SPOT:
            return self._ws_client_spot
        elif product_type == KrakenProductType.FUTURES:
            return self._ws_client_futures
        return None

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        self._cache_instruments()
        self._send_all_instruments_to_data_engine()

        instruments = self.instrument_provider.instruments_pyo3()

        # Connect spot WebSocket if configured
        if self._ws_client_spot is not None:
            await self._ws_client_spot.connect(
                instruments,
                self._handle_msg,
            )
            await self._ws_client_spot.wait_until_active(timeout_secs=10.0)
            self._ws_client_spot_connected = True
            self._log.info(f"Connected to spot websocket {self._ws_client_spot.url}", LogColor.BLUE)

        # Connect futures WebSocket if configured
        if self._ws_client_futures is not None:
            instruments_pyo3 = self.instrument_provider.instruments_pyo3()
            await self._ws_client_futures.connect(instruments_pyo3, self._handle_msg)
            self._ws_client_futures_connected = True
            self._log.info(f"Connected to futures websocket {self._ws_client_futures.url}", LogColor.BLUE)

        if self._config.update_instruments_interval_mins:
            self._update_instruments_task = self.create_task(
                self._update_instruments(self._config.update_instruments_interval_mins),
            )

    async def _disconnect(self) -> None:
        if self._http_client_spot is not None:
            self._http_client_spot.cancel_all_requests()
        if self._http_client_futures is not None:
            self._http_client_futures.cancel_all_requests()

        if self._update_instruments_task:
            self._log.debug("Canceling task 'update_instruments'")
            self._update_instruments_task.cancel()
            self._update_instruments_task = None

        # Delay to allow websocket to send any unsubscribe messages
        await asyncio.sleep(1.0)

        # Shutdown spot websocket
        if self._ws_client_spot is not None and not self._ws_client_spot.is_closed():
            self._log.info("Disconnecting spot websocket")
            await self._ws_client_spot.close()
            self._log.info(
                f"Disconnected from {self._ws_client_spot.url}",
                LogColor.BLUE,
            )

        # Shutdown futures websocket
        if (
            self._ws_client_futures is not None
            and self._ws_client_futures_connected
            and not self._ws_client_futures.is_closed()
        ):
            self._log.info("Disconnecting futures websocket")
            await self._ws_client_futures.close()
            self._ws_client_futures_connected = False
            self._log.info(
                f"Disconnected from {self._ws_client_futures.url}",
                LogColor.BLUE,
            )

        # Cancel any pending async futures
        await cancel_tasks_with_timeout(
            self._ws_client_async_futures,
            self._log,
            timeout_secs=DEFAULT_FUTURE_CANCELLATION_TIMEOUT,
        )
        self._ws_client_async_futures.clear()

    def _determine_ws_url(self, config: KrakenDataClientConfig) -> str:
        if config.base_url_ws:
            return config.base_url_ws

        # Derive WebSocket URL from environment and product type
        environment = config.environment or KrakenEnvironment.MAINNET
        product_types = config.product_types or (KrakenProductType.SPOT,)
        primary_product_type = product_types[0]

        return nautilus_pyo3.get_kraken_ws_public_url(primary_product_type, environment)

    def _cache_instruments(self) -> None:
        # Ensures instrument definitions are available for correct
        # price and size precisions when parsing responses
        instruments_pyo3 = self.instrument_provider.instruments_pyo3()

        for inst in instruments_pyo3:
            # Cache in the appropriate HTTP client based on instrument type
            client = self._get_http_client_for_symbol(str(inst.raw_symbol))
            if client:
                client.cache_instrument(inst)

        self._log.debug("Cached instruments", LogColor.MAGENTA)

    def _send_all_instruments_to_data_engine(self) -> None:
        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)

        for currency in self._instrument_provider.currencies().values():
            self._cache.add_currency(currency)

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        if command.book_type != BookType.L2_MBP:
            self._log.warning(
                f"Book type {book_type_to_str(command.book_type)} not supported by Kraken, skipping subscription",
            )
            return

        if command.depth not in (0, 10, 25, 100, 500, 1000):
            self._log.error(
                "Cannot subscribe to order book deltas: "
                f"invalid `depth`, was {command.depth}; "
                "valid depths are 0 (default 10), 10, 25, 100, 500, or 1000",
            )
            return

        symbol = command.instrument_id.symbol.value
        ws_client = self._get_ws_client_for_symbol(symbol)
        if ws_client is None:
            self._log.error(f"No WebSocket client configured for {command.instrument_id}")
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        depth = command.depth if command.depth != 0 else 10

        await ws_client.subscribe_book(pyo3_instrument_id, depth)

    async def _subscribe_order_book_snapshots(self, command: SubscribeOrderBook) -> None:
        if command.book_type != BookType.L2_MBP:
            self._log.warning(
                f"Book type {book_type_to_str(command.book_type)} not supported by Kraken, skipping subscription",
            )
            return

        if command.depth not in (0, 10, 25, 100, 500, 1000):
            self._log.error(
                "Cannot subscribe to order book snapshots: "
                f"invalid `depth`, was {command.depth}; "
                "valid depths are 0 (default 10), 10, 25, 100, 500, or 1000",
            )
            return

        symbol = command.instrument_id.symbol.value
        ws_client = self._get_ws_client_for_symbol(symbol)
        if ws_client is None:
            self._log.error(f"No WebSocket client configured for {command.instrument_id}")
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        depth = command.depth if command.depth != 0 else 10

        await ws_client.subscribe_book(pyo3_instrument_id, depth)

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        symbol = command.instrument_id.symbol.value
        ws_client = self._get_ws_client_for_symbol(symbol)
        if ws_client is None:
            self._log.error(f"No WebSocket client configured for {command.instrument_id}")
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await ws_client.subscribe_quotes(pyo3_instrument_id)

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        symbol = command.instrument_id.symbol.value
        ws_client = self._get_ws_client_for_symbol(symbol)
        if ws_client is None:
            self._log.error(f"No WebSocket client configured for {command.instrument_id}")
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await ws_client.subscribe_trades(pyo3_instrument_id)

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        self._log.error(
            f"Cannot subscribe to {command.bar_type} bars: "
            f"WebSocket bar streaming not yet implemented. "
            f"Use request_bars for historical bar data instead.",
        )

    async def _subscribe_instruments(self, command: SubscribeInstruments) -> None:
        if self._config.update_instruments_interval_mins:
            self._log.info(
                f"Kraken does not have an instruments channel, instrument updates are handled by "
                f"polling task running every {self._config.update_instruments_interval_mins} minutes",
                LogColor.BLUE,
            )
        else:
            self._log.warning(
                "Instruments subscription requested but update_instruments_interval_mins is not configured",
            )

    async def _subscribe_instrument(self, command: SubscribeInstrument) -> None:
        if self._config.update_instruments_interval_mins:
            self._log.info(
                f"Kraken does not have an instruments channel, instrument updates are handled by "
                f"polling task running every {self._config.update_instruments_interval_mins} minutes",
                LogColor.BLUE,
            )
        else:
            self._log.warning(
                "Instrument subscription requested but update_instruments_interval_mins is not configured",
            )

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        symbol = command.instrument_id.symbol.value
        ws_client = self._get_ws_client_for_symbol(symbol)
        if ws_client is None:
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await ws_client.unsubscribe_book(pyo3_instrument_id)

    async def _unsubscribe_order_book_snapshots(self, command: UnsubscribeOrderBook) -> None:
        symbol = command.instrument_id.symbol.value
        ws_client = self._get_ws_client_for_symbol(symbol)
        if ws_client is None:
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await ws_client.unsubscribe_book(pyo3_instrument_id)

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        symbol = command.instrument_id.symbol.value
        ws_client = self._get_ws_client_for_symbol(symbol)
        if ws_client is None:
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await ws_client.unsubscribe_quotes(pyo3_instrument_id)

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        symbol = command.instrument_id.symbol.value
        ws_client = self._get_ws_client_for_symbol(symbol)
        if ws_client is None:
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await ws_client.unsubscribe_trades(pyo3_instrument_id)

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        # Bar subscriptions are not supported, nothing to unsubscribe
        pass

    async def _unsubscribe_instruments(self, command: UnsubscribeInstruments) -> None:
        # Instruments are updated via polling task, no WebSocket unsubscribe needed
        pass

    async def _unsubscribe_instrument(self, command: UnsubscribeInstrument) -> None:
        # Instruments are updated via polling task, no WebSocket unsubscribe needed
        pass

    async def _ensure_futures_ws_connected(self) -> None:
        if self._ws_client_futures is None:
            self._log.warning("Futures WebSocket not configured")
            return

        if self._ws_client_futures_connected:
            return

        self._log.info("Connecting futures WebSocket (lazy)", LogColor.BLUE)

        # Get instruments for price precision lookup
        instruments_pyo3 = self.instrument_provider.instruments_pyo3()

        await self._ws_client_futures.connect(instruments_pyo3, self._handle_msg)
        self._ws_client_futures_connected = True

        self._log.info(
            f"Connected to futures websocket {self._ws_client_futures.url}",
            LogColor.BLUE,
        )

    async def _subscribe_mark_prices(self, command: SubscribeMarkPrices) -> None:
        instrument_id = command.instrument_id
        symbol = instrument_id.symbol.value
        product_type = nautilus_pyo3.kraken_product_type_from_symbol(symbol)

        if product_type != KrakenProductType.FUTURES:
            self._log.warning(
                f"Mark price subscription not supported for spot instrument {instrument_id}",
            )
            return

        if self._ws_client_futures is None:
            self._log.warning("Futures WebSocket not configured for mark price subscription")
            return

        await self._ensure_futures_ws_connected()
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(instrument_id.value)
        await self._ws_client_futures.subscribe_mark_price(pyo3_instrument_id)
        self._log.info(f"Subscribed to mark price for {instrument_id}", LogColor.BLUE)

    async def _subscribe_index_prices(self, command: SubscribeIndexPrices) -> None:
        instrument_id = command.instrument_id
        symbol = instrument_id.symbol.value
        product_type = nautilus_pyo3.kraken_product_type_from_symbol(symbol)

        if product_type != KrakenProductType.FUTURES:
            self._log.warning(
                f"Index price subscription not supported for spot instrument {instrument_id}",
            )
            return

        if self._ws_client_futures is None:
            self._log.warning("Futures WebSocket not configured for index price subscription")
            return

        await self._ensure_futures_ws_connected()
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(instrument_id.value)
        await self._ws_client_futures.subscribe_index_price(pyo3_instrument_id)
        self._log.info(f"Subscribed to index price for {instrument_id}", LogColor.BLUE)

    async def _unsubscribe_mark_prices(self, command: UnsubscribeMarkPrices) -> None:
        instrument_id = command.instrument_id
        symbol = instrument_id.symbol.value
        product_type = nautilus_pyo3.kraken_product_type_from_symbol(symbol)

        if product_type != KrakenProductType.FUTURES:
            return

        if self._ws_client_futures is None or not self._ws_client_futures_connected:
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(instrument_id.value)
        await self._ws_client_futures.unsubscribe_mark_price(pyo3_instrument_id)
        self._log.info(f"Unsubscribed from mark price for {instrument_id}", LogColor.BLUE)

    async def _unsubscribe_index_prices(self, command: UnsubscribeIndexPrices) -> None:
        instrument_id = command.instrument_id
        symbol = instrument_id.symbol.value
        product_type = nautilus_pyo3.kraken_product_type_from_symbol(symbol)

        if product_type != KrakenProductType.FUTURES:
            return

        if self._ws_client_futures is None or not self._ws_client_futures_connected:
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(instrument_id.value)
        await self._ws_client_futures.unsubscribe_index_price(pyo3_instrument_id)
        self._log.info(f"Unsubscribed from index price for {instrument_id}", LogColor.BLUE)

    async def _request_instruments(self, request: RequestInstruments) -> None:
        all_pyo3_instruments = []

        # Request instruments from all configured HTTP clients
        if self._http_client_spot is not None:
            pyo3_instruments = await self._http_client_spot.request_instruments()
            all_pyo3_instruments.extend(pyo3_instruments)
        if self._http_client_futures is not None:
            pyo3_instruments = await self._http_client_futures.request_instruments()
            all_pyo3_instruments.extend(pyo3_instruments)

        instruments = []
        for pyo3_instrument in all_pyo3_instruments:
            if isinstance(pyo3_instrument, KRAKEN_INSTRUMENT_TYPES):
                self._handle_instrument_update(pyo3_instrument)
            instrument = transform_instrument_from_pyo3(pyo3_instrument)
            instruments.append(instrument)

        self._handle_instruments(
            request.venue,
            instruments,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    async def _request_instrument(self, request: RequestInstrument) -> None:
        symbol = request.instrument_id.symbol.value
        client = self._get_http_client_for_symbol(symbol)

        if client is None:
            self._log.error(f"No HTTP client for instrument {request.instrument_id}")
            return

        pyo3_instruments = await client.request_instruments()
        for pyo3_instrument in pyo3_instruments:
            pyo3_instrument_id = pyo3_instrument.id
            if pyo3_instrument_id == nautilus_pyo3.InstrumentId.from_str(
                request.instrument_id.value,
            ):
                if isinstance(pyo3_instrument, KRAKEN_INSTRUMENT_TYPES):
                    self._handle_instrument_update(pyo3_instrument)
                instrument = transform_instrument_from_pyo3(pyo3_instrument)
                self._handle_instrument(
                    instrument,
                    request.id,
                    request.start,
                    request.end,
                    request.params,
                )
                return

        self._log.warning(f"Instrument {request.instrument_id} not found")

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        symbol = request.instrument_id.symbol.value
        client = self._get_http_client_for_symbol(symbol)

        if client is None:
            self._log.error(f"No HTTP client for instrument {request.instrument_id}")
            return

        limit = request.limit or None
        if limit is not None and limit > 1000:
            self._log.warning(
                f"Kraken limit {limit} exceeds maximum of 1000, clamping",
            )
            limit = 1000

        # Get nanosecond timestamps directly from request
        start = request.start.value if request.start else None
        end = request.end.value if request.end else None

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(request.instrument_id.value)

        try:
            pyo3_trades = await client.request_trades(
                instrument_id=pyo3_instrument_id,
                start=start,
                end=end,
                limit=limit,
            )
        except Exception as e:
            self._log.exception(
                f"Failed to request trades for {request.instrument_id}",
                e,
            )
            return

        trades = TradeTick.from_pyo3_list(pyo3_trades)

        self._handle_trade_ticks(
            request.instrument_id,
            trades,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    async def _request_bars(self, request: RequestBars) -> None:
        bar_type = request.bar_type
        symbol = bar_type.instrument_id.symbol.value
        client = self._get_http_client_for_symbol(symbol)

        if client is None:
            self._log.error(f"No HTTP client for instrument {bar_type.instrument_id}")
            return

        limit = request.limit or None
        if limit is not None and limit > 720:
            self._log.warning(
                f"Kraken bar limit {limit} exceeds maximum of 720, clamping",
            )
            limit = 720

        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(bar_type))

        # Get nanosecond timestamps directly from request
        start = request.start.value if request.start else None
        end = request.end.value if request.end else None

        try:
            pyo3_bars = await client.request_bars(
                bar_type=pyo3_bar_type,
                start=start,
                end=end,
                limit=limit,
            )
        except Exception as e:
            self._log.exception(f"Failed to request bars for {bar_type}", e)
            return

        bars = Bar.from_pyo3_list(pyo3_bars)

        self._handle_bars(
            bar_type,
            bars,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    async def _update_instruments(self, interval_mins: int) -> None:
        while True:
            try:
                self._log.debug(
                    f"Scheduled task 'update_instruments' to run in {interval_mins} minutes",
                )
                await asyncio.sleep(interval_mins * 60)
                await self._instrument_provider.initialize(reload=True)

                # Refresh HTTP/WS instrument caches with reloaded definitions
                self._cache_instruments()

                self._send_all_instruments_to_data_engine()
            except asyncio.CancelledError:
                self._log.debug("Canceled task 'update_instruments'")
                return
            except Exception as e:
                self._log.error(f"Error updating instruments: {e}")

    def _handle_msg(self, msg: Any) -> None:
        try:
            if nautilus_pyo3.is_pycapsule(msg):
                # The capsule will fall out of scope at the end of this method,
                # and eventually be garbage collected. The contained pointer
                # to `Data` is still owned and managed by Rust.
                data = capsule_to_data(msg)
                self._handle_data(data)
            elif isinstance(msg, KRAKEN_INSTRUMENT_TYPES):
                self._handle_instrument_update(msg)
            else:
                self._log.warning(f"Cannot handle message {msg}, not implemented")
        except Exception as e:
            self._log.exception("Error handling websocket message", e)

    def _handle_instrument_update(self, pyo3_instrument: KrakenInstrument) -> None:
        client = self._get_http_client_for_symbol(str(pyo3_instrument.raw_symbol))
        if client:
            client.cache_instrument(pyo3_instrument)

        if self._ws_client_spot is not None:
            self._ws_client_spot.cache_instrument(pyo3_instrument)

        if self._ws_client_futures is not None and self._ws_client_futures_connected:
            self._ws_client_futures.cache_instrument(pyo3_instrument)

        instrument = transform_instrument_from_pyo3(pyo3_instrument)

        self._handle_data(instrument)
