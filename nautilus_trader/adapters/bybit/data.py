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

import pandas as pd

from nautilus_trader.adapters.bybit.config import BybitDataClientConfig
from nautilus_trader.adapters.bybit.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.providers import BybitInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.datetime import ensure_pydatetime_utc
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeFundingRates
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeFundingRates
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.cancellation import DEFAULT_FUTURE_CANCELLATION_TIMEOUT
from nautilus_trader.live.cancellation import cancel_tasks_with_timeout
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import FundingRateUpdate
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import book_type_to_str
from nautilus_trader.model.identifiers import ClientId


class BybitDataClient(LiveMarketDataClient):
    """
    Provides a data client for the Bybit centralized crypto exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.BybitHttpClient
        The Bybit HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : BybitInstrumentProvider
        The instrument provider.
    config : BybitDataClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.BybitHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: BybitInstrumentProvider,
        config: BybitDataClientConfig,
        name: str | None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or BYBIT_VENUE.value),
            venue=BYBIT_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        self._instrument_provider: BybitInstrumentProvider = instrument_provider

        # Configuration
        self._config = config
        self._product_types = (
            list(config.product_types)
            if config.product_types
            else [
                nautilus_pyo3.BybitProductType.SPOT,
                nautilus_pyo3.BybitProductType.LINEAR,
                nautilus_pyo3.BybitProductType.INVERSE,
                nautilus_pyo3.BybitProductType.OPTION,
            ]
        )
        self._bars_timestamp_on_close = config.bars_timestamp_on_close

        self._log.info(f"Product types: {[str(p) for p in self._product_types]}", LogColor.BLUE)
        self._log.info(f"{config.update_instruments_interval_mins=}", LogColor.BLUE)
        self._log.info(f"{config.recv_window_ms=:_}", LogColor.BLUE)
        self._log.info(f"{config.bars_timestamp_on_close=}", LogColor.BLUE)
        self._log.info(f"{config.http_proxy_url=}", LogColor.BLUE)
        self._log.info(f"{config.ws_proxy_url=}", LogColor.BLUE)

        # HTTP API
        self._http_client = client
        masked_key = self._http_client.api_key_masked
        self._log.info(f"REST API key {masked_key}", LogColor.BLUE)

        # WebSocket API - create clients for each product type (public endpoints)
        self._ws_clients: dict[
            nautilus_pyo3.BybitProductType,
            nautilus_pyo3.BybitWebSocketClient,
        ] = {}
        self._ws_client_futures: set[asyncio.Future] = set()

        # Priority: demo > testnet > mainnet
        if config.demo:
            environment = nautilus_pyo3.BybitEnvironment.DEMO
        elif config.testnet:
            environment = nautilus_pyo3.BybitEnvironment.TESTNET
        else:
            environment = nautilus_pyo3.BybitEnvironment.MAINNET

        for product_type in self._product_types:
            ws_client = nautilus_pyo3.BybitWebSocketClient.new_public(
                product_type=product_type,
                environment=environment,
                url=config.base_url_http,
                heartbeat=None,
            )
            self._ws_clients[product_type] = ws_client

        self._depths: dict[nautilus_pyo3.InstrumentId, int] = {}
        self._quote_depths: dict[nautilus_pyo3.InstrumentId, int] = {}

        # Reference counting for ticker channel
        self._ticker_subscriptions: dict[nautilus_pyo3.InstrumentId, set[str]] = {}

        self._update_instruments_interval_mins: int | None = config.update_instruments_interval_mins
        self._update_instruments_task: asyncio.Task | None = None

    @property
    def instrument_provider(self) -> BybitInstrumentProvider:
        return self._instrument_provider

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        self._cache_instruments()
        self._send_all_instruments_to_data_engine()

        # Connect all websocket clients
        for product_type, ws_client in self._ws_clients.items():
            await ws_client.connect(callback=self._handle_msg)
            await ws_client.wait_until_active(timeout_secs=10.0)
            self._log.info(f"Connected to {product_type.name} websocket", LogColor.BLUE)

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

        # Delay to allow websocket to send any unsubscribe messages
        await asyncio.sleep(1.0)

        # Shutdown all websocket clients
        for product_type, ws_client in self._ws_clients.items():
            self._log.info(f"Disconnecting {product_type.name} websocket")
            await ws_client.close()
            self._log.info(f"Disconnected from {product_type.name} websocket", LogColor.BLUE)

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

        for inst in instruments_pyo3:
            self._http_client.cache_instrument(inst)

            for ws_client in self._ws_clients.values():
                ws_client.cache_instrument(inst)

        self._log.debug("Cached instruments", LogColor.MAGENTA)

    def _send_all_instruments_to_data_engine(self) -> None:
        for currency in self._instrument_provider.currencies().values():
            self._cache.add_currency(currency)

        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)

    def _get_ws_client_for_instrument(
        self,
        instrument_id: nautilus_pyo3.InstrumentId,
    ) -> nautilus_pyo3.BybitWebSocketClient:
        product_type = nautilus_pyo3.bybit_product_type_from_symbol(instrument_id.symbol.value)

        ws_client = self._ws_clients.get(product_type)
        if ws_client is None:
            raise ValueError(
                f"No WebSocket client configured for product type {product_type.name}",
            )

        return ws_client

    def _bar_spec_to_bybit_interval(self, bar_spec) -> str:
        return nautilus_pyo3.bybit_bar_spec_to_interval(
            bar_spec.aggregation,
            bar_spec.step,
        )

    async def _update_instruments(self, interval_mins: int) -> None:
        while True:
            try:
                await asyncio.sleep(interval_mins * 60)
                await self._instrument_provider.initialize(reload=True)
                self._cache_instruments()
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

    async def _subscribe_instruments(self, command) -> None:
        if self._update_instruments_interval_mins:
            self._log.info(
                f"Bybit does not have an instruments channel, instrument updates are handled by "
                f"polling task running every {self._update_instruments_interval_mins} minutes",
                LogColor.BLUE,
            )
        else:
            self._log.warning(
                "Instruments subscription requested but update_instruments_interval_mins is not configured",
            )

    async def _subscribe_instrument(self, command) -> None:
        if self._update_instruments_interval_mins:
            self._log.info(
                f"Bybit does not have an instruments channel, instrument updates are handled by "
                f"polling task running every {self._update_instruments_interval_mins} minutes",
                LogColor.BLUE,
            )
        else:
            self._log.warning(
                "Instrument subscription requested but update_instruments_interval_mins is not configured",
            )

    async def _unsubscribe_instruments(self, command) -> None:
        # Instruments are updated via polling task, no WebSocket unsubscribe needed
        pass

    async def _unsubscribe_instrument(self, command) -> None:
        # Instruments are updated via polling task, no WebSocket unsubscribe needed
        pass

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        if command.book_type != BookType.L2_MBP:
            self._log.warning(
                f"Book type {book_type_to_str(command.book_type)} not supported by Bybit, skipping subscription",
            )
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        depth = command.depth if command.depth != 0 else 50

        # Store depth for later unsubscribe
        self._depths[pyo3_instrument_id] = depth

        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.subscribe_orderbook(pyo3_instrument_id, depth)

    async def _subscribe_order_book_snapshots(self, command: SubscribeOrderBook) -> None:
        # Bybit doesn't differentiate between snapshots and deltas at subscription level
        await self._subscribe_order_book_deltas(command)

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        product_type = nautilus_pyo3.bybit_product_type_from_symbol(
            pyo3_instrument_id.symbol.value,
        )
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)

        # SPOT ticker channel doesn't include bid/ask, use orderbook depth=1
        if product_type == nautilus_pyo3.BybitProductType.SPOT:
            depth = 1
            self._quote_depths[pyo3_instrument_id] = depth
            await ws_client.subscribe_orderbook(pyo3_instrument_id, depth)
        else:
            # Reference counting: only subscribe if first user of ticker channel
            if pyo3_instrument_id not in self._ticker_subscriptions:
                self._ticker_subscriptions[pyo3_instrument_id] = set()
                await ws_client.subscribe_ticker(pyo3_instrument_id)
            self._ticker_subscriptions[pyo3_instrument_id].add("quotes")

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.subscribe_trades(pyo3_instrument_id)

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
            command.bar_type.instrument_id.value,
        )
        interval = self._bar_spec_to_bybit_interval(command.bar_type.spec)
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.subscribe_klines(pyo3_instrument_id, interval)

    async def _subscribe_funding_rates(self, command: SubscribeFundingRates) -> None:
        # Bybit doesn't have a separate funding rate subscription
        # Funding rate data comes through ticker subscriptions for perpetual instruments
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        product_type = nautilus_pyo3.bybit_product_type_from_symbol(
            pyo3_instrument_id.symbol.value,
        )

        if product_type == nautilus_pyo3.BybitProductType.SPOT:
            self._log.warning(
                f"Cannot subscribe to funding rates for SPOT instrument {command.instrument_id}",
            )
            return

        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)

        # Reference counting: only subscribe if first user of ticker channel
        if pyo3_instrument_id not in self._ticker_subscriptions:
            self._ticker_subscriptions[pyo3_instrument_id] = set()
            await ws_client.subscribe_ticker(pyo3_instrument_id)
        self._ticker_subscriptions[pyo3_instrument_id].add("funding")

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)

        # Get depth from subscription tracking (default to 1 if not found)
        depth = self._depths.get(pyo3_instrument_id, 1)

        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.unsubscribe_orderbook(pyo3_instrument_id, depth)

        # Remove from tracking
        self._depths.pop(pyo3_instrument_id, None)

    async def _unsubscribe_order_book_snapshots(self, command: UnsubscribeOrderBook) -> None:
        await self._unsubscribe_order_book_deltas(command)

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        product_type = nautilus_pyo3.bybit_product_type_from_symbol(
            pyo3_instrument_id.symbol.value,
        )
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)

        if product_type == nautilus_pyo3.BybitProductType.SPOT:
            depth = self._quote_depths.get(pyo3_instrument_id, 1)
            await ws_client.unsubscribe_orderbook(pyo3_instrument_id, depth)
            self._quote_depths.pop(pyo3_instrument_id, None)
        else:
            # Reference counting: only unsubscribe if last user of ticker channel
            if pyo3_instrument_id in self._ticker_subscriptions:
                self._ticker_subscriptions[pyo3_instrument_id].discard("quotes")
                if not self._ticker_subscriptions[pyo3_instrument_id]:
                    await ws_client.unsubscribe_ticker(pyo3_instrument_id)
                    del self._ticker_subscriptions[pyo3_instrument_id]

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.unsubscribe_trades(pyo3_instrument_id)

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
            command.bar_type.instrument_id.value,
        )
        interval = self._bar_spec_to_bybit_interval(command.bar_type.spec)
        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)
        await ws_client.unsubscribe_klines(pyo3_instrument_id, interval)

    async def _unsubscribe_funding_rates(self, command: UnsubscribeFundingRates) -> None:
        # Bybit doesn't have a separate funding rate subscription
        # Unsubscribe from ticker which includes funding rate updates
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        product_type = nautilus_pyo3.bybit_product_type_from_symbol(
            pyo3_instrument_id.symbol.value,
        )

        if product_type == nautilus_pyo3.BybitProductType.SPOT:
            return

        ws_client = self._get_ws_client_for_instrument(pyo3_instrument_id)

        # Reference counting: only unsubscribe if last user of ticker channel
        if pyo3_instrument_id in self._ticker_subscriptions:
            self._ticker_subscriptions[pyo3_instrument_id].discard("funding")
            if not self._ticker_subscriptions[pyo3_instrument_id]:
                await ws_client.unsubscribe_ticker(pyo3_instrument_id)
                del self._ticker_subscriptions[pyo3_instrument_id]

    async def _request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        self._log.error(
            "Cannot request historical quotes: not published by Bybit",
        )

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        limit = request.limit

        if limit == 0 or limit > 1000:
            limit = 1000

        # Bybit's recent-trade endpoint does not support start/end time parameters
        # It always returns the most recent trades regardless of time range specified
        # We fetch recent trades and filter client-side, but this only works for very recent windows
        time_ago = self._clock.utc_now() - request.start

        # Hard error for requests clearly outside the "recent trades" window
        if time_ago > pd.Timedelta(hours=1):
            self._log.error(
                f"Cannot request trades from {time_ago.total_seconds() / 3600:.1f}h ago: "
                f"Bybit only provides recent trades (typically last few minutes). "
                f"Use bars/klines for historical data.",
            )
            return

        # Warn if requesting data that might not be in the recent trades window
        if time_ago > pd.Timedelta(minutes=1):
            self._log.warning(
                f"Requesting trades from {time_ago.total_seconds() / 60:.1f} minutes ago. "
                f"Bybit API only returns recent trades; older data may be unavailable. "
                f"Consider using bars for historical data.",
            )

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(request.instrument_id.value)
        product_type = nautilus_pyo3.bybit_product_type_from_symbol(
            pyo3_instrument_id.symbol.value,
        )

        pyo3_trades = await self._http_client.request_trades(
            product_type=product_type,
            instrument_id=pyo3_instrument_id,
            limit=limit,
        )
        trades = TradeTick.from_pyo3_list(pyo3_trades)

        # Filter trades to only include those within the requested time window
        # Bybit API returns recent trades regardless of time params, so we filter client-side
        start_ns = request.start.value
        end_ns = request.end.value
        filtered_trades = [trade for trade in trades if start_ns <= trade.ts_event <= end_ns]

        if len(filtered_trades) < len(trades):
            self._log.debug(
                f"Filtered {len(trades) - len(filtered_trades)} trades outside "
                f"requested window [{request.start}, {request.end}]",
            )

        self._handle_trade_ticks(
            request.instrument_id,
            filtered_trades,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    async def _request_bars(self, request: RequestBars) -> None:
        if request.bar_type.is_internally_aggregated():
            self._log.error(
                f"Cannot request {request.bar_type} bars: "
                f"only historical bars with EXTERNAL aggregation available from Bybit",
            )
            return

        if not request.bar_type.spec.is_time_aggregated():
            self._log.error(
                f"Cannot request {request.bar_type} bars: only time bars are aggregated by Bybit",
            )
            return

        if request.bar_type.spec.price_type != PriceType.LAST:
            self._log.error(
                f"Cannot request {request.bar_type} bars: "
                f"only historical bars for LAST price type available from Bybit",
            )
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
            request.bar_type.instrument_id.value,
        )
        product_type = nautilus_pyo3.bybit_product_type_from_symbol(
            pyo3_instrument_id.symbol.value,
        )
        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(request.bar_type))

        self._log.debug(
            f"Requesting klines start={request.start}, end={request.end}, {request.limit=}",
        )

        pyo3_bars = await self._http_client.request_bars(
            product_type=product_type,
            bar_type=pyo3_bar_type,
            start=ensure_pydatetime_utc(request.start),
            end=ensure_pydatetime_utc(request.end),
            limit=request.limit or 200,
            timestamp_on_close=self._bars_timestamp_on_close,
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

    def _handle_msg(self, msg: Any) -> None:
        try:
            # Handle pycapsule data from Rust (market data)
            if nautilus_pyo3.is_pycapsule(msg):
                # The capsule will fall out of scope at the end of this method,
                # and eventually be garbage collected. The contained pointer
                # to `Data` is still owned and managed by Rust.
                data = capsule_to_data(msg)
                self._handle_data(data)
                return

            if isinstance(msg, nautilus_pyo3.FundingRateUpdate):
                data = FundingRateUpdate.from_pyo3(msg)
                self._handle_data(data)
                return

            msg_str = msg.decode("utf-8") if isinstance(msg, bytes) else str(msg)
            if msg_str:
                self._log.debug(f"WebSocket message: {msg_str}")
        except Exception as e:
            self._log.exception("Error handling websocket message", e)
