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
Data client for the dYdX v4 decentralized crypto exchange.

This client uses the Rust-backed HTTP and WebSocket clients for market data.

"""

import asyncio

from nautilus_trader.adapters.dydx_v4.common.urls import get_ws_url
from nautilus_trader.adapters.dydx_v4.config import DYDXv4DataClientConfig
from nautilus_trader.adapters.dydx_v4.constants import DYDX_VENUE
from nautilus_trader.adapters.dydx_v4.providers import DYDXv4InstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestOrderBookSnapshot
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeFundingRates
from nautilus_trader.data.messages import SubscribeIndexPrices
from nautilus_trader.data.messages import SubscribeInstrument
from nautilus_trader.data.messages import SubscribeInstrumentClose
from nautilus_trader.data.messages import SubscribeInstruments
from nautilus_trader.data.messages import SubscribeInstrumentStatus
from nautilus_trader.data.messages import SubscribeMarkPrices
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeFundingRates
from nautilus_trader.data.messages import UnsubscribeIndexPrices
from nautilus_trader.data.messages import UnsubscribeInstrument
from nautilus_trader.data.messages import UnsubscribeInstrumentClose
from nautilus_trader.data.messages import UnsubscribeInstruments
from nautilus_trader.data.messages import UnsubscribeInstrumentStatus
from nautilus_trader.data.messages import UnsubscribeMarkPrices
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.cancellation import DEFAULT_FUTURE_CANCELLATION_TIMEOUT
from nautilus_trader.live.cancellation import cancel_tasks_with_timeout
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import bar_aggregation_to_str
from nautilus_trader.model.enums import book_type_to_str
from nautilus_trader.model.identifiers import ClientId


# Mapping of Nautilus bar aggregation/step to dYdX resolution strings
BAR_RESOLUTION_MAP = {
    (1, "MINUTE"): "1MIN",
    (5, "MINUTE"): "5MINS",
    (15, "MINUTE"): "15MINS",
    (30, "MINUTE"): "30MINS",
    (1, "HOUR"): "1HOUR",
    (4, "HOUR"): "4HOURS",
    (1, "DAY"): "1DAY",
}


class DYDXv4DataClient(LiveMarketDataClient):
    """
    Provides a data client for the dYdX v4 decentralized crypto exchange.

    This client uses Rust-backed HTTP and WebSocket clients for market data.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.DydxHttpClient
        The dYdX HTTP client (Rust-backed).
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : DYDXv4InstrumentProvider
        The instrument provider.
    config : DYDXv4DataClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.DydxHttpClient,  # type: ignore[name-defined]
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: DYDXv4InstrumentProvider,
        config: DYDXv4DataClientConfig,
        name: str | None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or DYDX_VENUE.value),
            venue=DYDX_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        self._instrument_provider: DYDXv4InstrumentProvider = instrument_provider

        # Configuration
        self._config = config
        self._log.info(f"{config.is_testnet=}", LogColor.BLUE)
        self._log.info(f"{config.update_instruments_interval_mins=}", LogColor.BLUE)
        self._log.info(f"{config.max_retries=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_initial_ms=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_max_ms=}", LogColor.BLUE)

        # HTTP API
        self._http_client = client

        # WebSocket API (using public client for market data)
        ws_url = config.base_url_ws or get_ws_url(is_testnet=config.is_testnet)
        self._ws_client = nautilus_pyo3.DydxWebSocketClient.new_public(  # type: ignore[attr-defined]
            url=ws_url,
            heartbeat=20,
        )
        self._ws_client_futures: set[asyncio.Future] = set()

    @property
    def instrument_provider(self) -> DYDXv4InstrumentProvider:
        return self._instrument_provider

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        self._cache_instruments()
        self._send_all_instruments_to_data_engine()

        instruments = self.instrument_provider.instruments_pyo3()

        await self._ws_client.connect(
            instruments=instruments,
            callback=self._handle_msg,
        )

        # Wait for connection to be established
        await self._ws_client.wait_until_active(timeout_secs=30.0)
        self._log.info(f"Connected to WebSocket {self._ws_client.py_url}", LogColor.BLUE)

        # Subscribe to markets channel for instrument updates (mark prices, funding rates, etc.)
        await self._ws_client.subscribe_markets()

    async def _disconnect(self) -> None:
        # Delay to allow websocket to send any unsubscribe messages
        await asyncio.sleep(1.0)

        # Shutdown websocket
        if not self._ws_client.is_closed():
            self._log.info("Disconnecting WebSocket")

            await self._ws_client.disconnect()

            self._log.info(
                f"Disconnected from {self._ws_client.py_url}",
                LogColor.BLUE,
            )

        # Cancel any pending futures
        await cancel_tasks_with_timeout(
            self._ws_client_futures,
            self._log,
            timeout_secs=DEFAULT_FUTURE_CANCELLATION_TIMEOUT,
        )

        self._ws_client_futures.clear()

    def _cache_instruments(self) -> None:
        # Ensures instrument definitions are available for correct
        # price and size precisions when parsing responses
        instruments_pyo3 = self.instrument_provider.instruments_pyo3()
        self._http_client.cache_instruments(instruments_pyo3)

        self._log.debug(f"Cached {len(instruments_pyo3)} instruments", LogColor.MAGENTA)

    def _send_all_instruments_to_data_engine(self) -> None:
        for currency in self._instrument_provider.currencies().values():
            self._cache.add_currency(currency)

        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)

    def _handle_msg(self, capsule: object) -> None:
        try:
            data = capsule_to_data(capsule)
            self._handle_data(data)
        except Exception as e:
            self._log.error(f"Error handling WebSocket message: {e}")

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    async def _subscribe_instruments(self, command: SubscribeInstruments) -> None:
        # The WebSocket client subscribes to markets channel automatically on connect
        pass

    async def _subscribe_instrument(self, command: SubscribeInstrument) -> None:
        # dYdX markets channel provides all instrument updates, no per-instrument subscription
        pass

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        if command.book_type != BookType.L2_MBP:
            self._log.warning(
                f"Book type {book_type_to_str(command.book_type)} not supported by dYdX, skipping subscription",
            )
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_orderbook(pyo3_instrument_id)

    async def _subscribe_order_book_snapshots(self, command: SubscribeOrderBook) -> None:
        # dYdX provides deltas with initial snapshot, no separate snapshot subscription
        await self._subscribe_order_book_deltas(command)

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        # dYdX doesn't have a dedicated quote tick channel
        # Quotes are synthesized from orderbook data
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_orderbook(pyo3_instrument_id)

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_trades(pyo3_instrument_id)

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        bar_type = command.bar_type
        spec = bar_type.spec

        # Map to dYdX resolution
        aggregation_str = bar_aggregation_to_str(spec.aggregation)
        key = (spec.step, aggregation_str)
        resolution = BAR_RESOLUTION_MAP.get(key)

        if resolution is None:
            self._log.error(
                f"Cannot subscribe to bars: unsupported aggregation "
                f"step={spec.step} aggregation={aggregation_str}",
            )
            return

        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(bar_type))
        await self._ws_client.subscribe_bars(pyo3_bar_type, resolution)

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_orderbook(pyo3_instrument_id)

    async def _unsubscribe_order_book_snapshots(self, command: UnsubscribeOrderBook) -> None:
        await self._unsubscribe_order_book_deltas(command)

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        # Quotes are synthesized from orderbook, unsubscribe orderbook
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_orderbook(pyo3_instrument_id)

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_trades(pyo3_instrument_id)

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        bar_type = command.bar_type
        spec = bar_type.spec

        aggregation_str = bar_aggregation_to_str(spec.aggregation)
        key = (spec.step, aggregation_str)
        resolution = BAR_RESOLUTION_MAP.get(key)

        if resolution is None:
            return

        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(bar_type))
        await self._ws_client.unsubscribe_bars(pyo3_bar_type, resolution)

    async def _unsubscribe_instruments(self, command: UnsubscribeInstruments) -> None:
        # Markets channel is always subscribed, no unsubscription needed
        pass

    async def _unsubscribe_instrument(self, command: UnsubscribeInstrument) -> None:
        # Markets channel is always subscribed, no per-instrument unsubscription
        pass

    # -- REQUESTS ---------------------------------------------------------------------------------

    async def _request_instrument(self, request: RequestInstrument) -> None:
        instrument = self._instrument_provider.find(request.instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find instrument for {request.instrument_id}")
            return
        self._handle_data_response(
            data_type=request.data_type,
            data=instrument,
            correlation_id=request.id,
        )

    async def _request_instruments(self, request: RequestInstruments) -> None:
        instruments = list(self._instrument_provider.get_all().values())
        self._handle_data_response(
            data_type=request.data_type,
            data=instruments,
            correlation_id=request.id,
        )

    async def _subscribe_mark_prices(self, command: SubscribeMarkPrices) -> None:
        # dYdX provides mark prices through the markets channel
        pass

    async def _unsubscribe_mark_prices(self, command: UnsubscribeMarkPrices) -> None:
        # Mark prices are part of markets channel, no separate unsubscription
        pass

    async def _subscribe_index_prices(self, command: SubscribeIndexPrices) -> None:
        # dYdX provides index prices through the markets channel
        pass

    async def _unsubscribe_index_prices(self, command: UnsubscribeIndexPrices) -> None:
        # Index prices are part of markets channel, no separate unsubscription
        pass

    async def _subscribe_funding_rates(self, command: SubscribeFundingRates) -> None:
        # dYdX provides funding rates through the markets channel
        pass

    async def _unsubscribe_funding_rates(self, command: UnsubscribeFundingRates) -> None:
        # Funding rates are part of markets channel, no separate unsubscription
        pass

    async def _subscribe_instrument_status(self, command: SubscribeInstrumentStatus) -> None:
        # dYdX provides instrument status through the markets channel
        pass

    async def _unsubscribe_instrument_status(self, command: UnsubscribeInstrumentStatus) -> None:
        # Instrument status is part of markets channel, no separate unsubscription
        pass

    async def _subscribe_instrument_close(self, command: SubscribeInstrumentClose) -> None:
        # dYdX does not support instrument close subscriptions (perpetuals only)
        self._log.warning("Instrument close subscriptions not supported by dYdX")

    async def _unsubscribe_instrument_close(self, command: UnsubscribeInstrumentClose) -> None:
        # dYdX does not support instrument close subscriptions
        pass

    async def _request_bars(self, request: RequestBars) -> None:
        bar_type = request.bar_type
        spec = bar_type.spec

        # Map to dYdX resolution
        aggregation_str = bar_aggregation_to_str(spec.aggregation)
        key = (spec.step, aggregation_str)
        resolution = BAR_RESOLUTION_MAP.get(key)

        if resolution is None:
            self._log.error(
                f"Cannot request bars: unsupported aggregation "
                f"step={spec.step} aggregation={aggregation_str}",
            )
            return

        # Format timestamps for dYdX API
        start_iso = request.start.isoformat() if request.start else None
        end_iso = request.end.isoformat() if request.end else None

        # Determine limit
        limit = request.limit if request.limit > 0 else None

        self._log.info(
            f"Request {bar_type} bars from {start_iso or 'start'} to {end_iso or 'end'}",
        )

        try:
            bars = await self._http_client.request_bars(
                bar_type=str(bar_type),
                resolution=resolution,
                limit=limit,
                start=start_iso,
                end=end_iso,
            )

            self._log.info(f"Received {len(bars)} bars for {bar_type}")

            self._handle_bars(
                bar_type,
                bars,
                request.id,
                request.start,
                request.end,
                request.params,
            )

        except Exception as e:
            self._log.error(f"Error requesting bars for {bar_type}: {e}")

    async def _request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        # dYdX does not publish historical quote tick data
        self._log.warning(
            "Cannot request historical quotes: not published by dYdX. "
            "Subscribe to order book for top-of-book data.",
        )

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        try:
            pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                request.instrument_id.value,
            )

            pyo3_trades = await self._http_client.request_trade_ticks(
                instrument_id=pyo3_instrument_id,
                limit=request.limit,
            )

            from nautilus_trader.model.data import TradeTick

            trades = TradeTick.from_pyo3_list(pyo3_trades)

            self._handle_trade_ticks(
                request.instrument_id,
                trades,
                request.id,
                request.start,
                request.end,
                request.params,
            )

        except Exception as e:
            self._log.error(f"Error requesting trade ticks for {request.instrument_id}: {e}")

    async def _request_order_book_snapshot(self, request: RequestOrderBookSnapshot) -> None:
        try:
            pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                request.instrument_id.value,
            )

            pyo3_deltas = await self._http_client.request_orderbook_snapshot(
                instrument_id=pyo3_instrument_id,
            )

            from nautilus_trader.model.data import OrderBookDeltas

            deltas = OrderBookDeltas.from_pyo3(pyo3_deltas)

            self._handle_order_book_deltas(deltas)

        except Exception as e:
            self._log.error(
                f"Error requesting order book snapshot for {request.instrument_id}: {e}",
            )
