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

import asyncio
from typing import Any

from nautilus_trader.adapters.deribit.config import DeribitDataClientConfig
from nautilus_trader.adapters.deribit.constants import DERIBIT_DATA_SESSION_NAME
from nautilus_trader.adapters.deribit.constants import DERIBIT_VENUE
from nautilus_trader.adapters.deribit.constants import DERIBIT_WS_HEARTBEAT_SECS
from nautilus_trader.adapters.deribit.providers import DeribitInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.cache.transformers import transform_instrument_from_pyo3
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.secure import mask_api_key
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.datetime import ensure_pydatetime_utc
from nautilus_trader.core.nautilus_pyo3 import DeribitCurrency
from nautilus_trader.core.nautilus_pyo3 import DeribitUpdateInterval
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestOrderBookSnapshot
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
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import FundingRateUpdate
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.model.enums import book_type_to_str
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument


class DeribitDataClient(LiveMarketDataClient):
    """
    Provides a data client for the Deribit centralized crypto derivatives exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.DeribitHttpClient
        The Deribit HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : DeribitInstrumentProvider
        The instrument provider.
    config : DeribitDataClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.DeribitHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: DeribitInstrumentProvider,
        config: DeribitDataClientConfig,
        name: str | None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or DERIBIT_VENUE.value),
            venue=DERIBIT_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        self._instrument_provider: DeribitInstrumentProvider = instrument_provider

        product_types = [k.name for k in config.product_types] if config.product_types else None

        # Configuration
        self._config = config
        self._log.info(f"config.product_types={product_types}", LogColor.BLUE)
        self._log.info(f"{config.is_testnet=}", LogColor.BLUE)
        self._log.info(f"{config.http_timeout_secs=}", LogColor.BLUE)
        self._log.info(f"{config.max_retries=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_initial_ms=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_max_ms=}", LogColor.BLUE)
        self._log.info(f"{config.update_instruments_interval_mins=}", LogColor.BLUE)

        # HTTP API
        self._http_client = client
        if config.api_key:
            masked_key = mask_api_key(config.api_key)
            self._log.info(f"REST API key {masked_key}", LogColor.BLUE)

        # WebSocket API
        ws_url = config.base_url_ws or nautilus_pyo3.get_deribit_ws_url(config.is_testnet)
        self._ws_client = nautilus_pyo3.DeribitWebSocketClient(
            url=ws_url,
            api_key=config.api_key,
            api_secret=config.api_secret,
            heartbeat_interval=DERIBIT_WS_HEARTBEAT_SECS,
            is_testnet=config.is_testnet,
        )
        self._ws_client_futures: set[asyncio.Future] = set()

        # Track book subscription depths for proper unsubscribe
        self._book_subscription_depths: dict[InstrumentId, int] = {}

    @property
    def instrument_provider(self) -> DeribitInstrumentProvider:
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
        self._log.info(f"Connected to WebSocket {self._ws_client.url}", LogColor.BLUE)

        # Authenticate if credentials are configured (required for raw streams)
        if self._ws_client.has_credentials():
            self._log.info("Authenticating WebSocket session for raw streams...")
            await self._ws_client.authenticate_session(DERIBIT_DATA_SESSION_NAME)
            self._log.info("WebSocket authenticated", LogColor.GREEN)

    async def _disconnect(self) -> None:
        # Delay to allow WebSocket to send any unsubscribe messages
        await asyncio.sleep(1.0)

        if not self._ws_client.is_closed():
            self._log.info("Disconnecting WebSocket")

            await self._ws_client.close()

            self._log.info(
                f"Disconnected from {self._ws_client.url}",
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
        for inst in instruments_pyo3:
            self._http_client.cache_instrument(inst)

        self._ws_client.cache_instruments(instruments_pyo3)

        self._log.debug("Cached instruments", LogColor.MAGENTA)

    def _send_all_instruments_to_data_engine(self) -> None:
        for currency in self._instrument_provider.currencies().values():
            self._cache.add_currency(currency)

        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)

    def _get_interval(self, params: dict[str, Any] | None) -> DeribitUpdateInterval | None:
        if params:
            interval_str = params.get("interval")
            if interval_str:
                return DeribitUpdateInterval.from_str(interval_str)

        # Default to Raw if authenticated, otherwise None (100ms default)
        if self._ws_client.is_authenticated():
            return DeribitUpdateInterval.RAW

        return None

    async def _subscribe_instruments(self, command: SubscribeInstruments) -> None:
        kind = "any"
        currency = "any"

        if command.params:
            kind = command.params.get("kind", "any")
            currency = command.params.get("currency", "any")

        self._log.info(f"Subscribing to instrument state changes: {kind}.{currency}")
        await self._ws_client.subscribe_instrument_state(kind, currency)

    async def _subscribe_instrument(self, command: SubscribeInstrument) -> None:
        symbol = command.instrument_id.symbol.value

        if "PERPETUAL" in symbol:
            kind = "future"
        elif symbol.endswith(("-C", "-P")):
            kind = "option"
        elif "_" in symbol and "-" not in symbol:
            kind = "spot"
        else:
            kind = "future"  # Futures with expiry dates like "BTC-28MAR25"

        # For instruments like "BTC-PERPETUAL", "BTC-28MAR25", "BTC_USDC"
        parts = symbol.replace("_", "-").split("-")
        currency = parts[0] if parts else "any"

        self._log.info(
            f"Subscribing to instrument state for {command.instrument_id} "
            f"(channel: instrument.state.{kind}.{currency})",
        )
        await self._ws_client.subscribe_instrument_state(kind, currency)

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        if command.book_type != BookType.L2_MBP:
            self._log.warning(
                f"Book type {book_type_to_str(command.book_type)} not supported by Deribit, skipping subscription",
            )
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        interval = self._get_interval(command.params)

        depth = command.depth or None
        if not depth and command.params:
            depth_str = command.params.get("depth")
            if depth_str:
                depth = int(depth_str)

        # Track depth for proper unsubscribe
        if depth:
            self._book_subscription_depths[command.instrument_id] = depth

        await self._ws_client.subscribe_book(pyo3_instrument_id, interval, depth)

    async def _subscribe_order_book_depth(self, command: SubscribeOrderBook) -> None:
        if command.book_type != BookType.L2_MBP:
            self._log.warning(
                f"Book type {book_type_to_str(command.book_type)} not supported by Deribit, skipping subscription",
            )
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        depth = command.depth or 10  # Default for OrderBookDepth10
        group = "none"
        interval = self._get_interval(command.params)

        # TODO: Standardize to validate instead of normalize
        # Rust layer normalizes depth to Deribit supported values (1, 10, 20)
        await self._ws_client.subscribe_book_grouped(pyo3_instrument_id, group, depth, interval)

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_quotes(pyo3_instrument_id)

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        interval = self._get_interval(command.params)
        await self._ws_client.subscribe_trades(pyo3_instrument_id, interval)

    async def _subscribe_mark_prices(self, command: SubscribeMarkPrices) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        interval = self._get_interval(command.params)
        interval_display = interval.name if interval else "100ms (default)"
        self._log.info(
            f"Subscribing to mark prices for {command.instrument_id} "
            f"(via ticker channel, interval: {interval_display})",
        )
        await self._ws_client.subscribe_ticker(pyo3_instrument_id, interval)

    async def _subscribe_index_prices(self, command: SubscribeIndexPrices) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        interval = self._get_interval(command.params)
        interval_display = interval.name if interval else "100ms (default)"
        self._log.info(
            f"Subscribing to index prices for {command.instrument_id} "
            f"(via ticker channel, interval: {interval_display})",
        )
        await self._ws_client.subscribe_ticker(pyo3_instrument_id, interval)

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(command.bar_type))
        await self._ws_client.subscribe_bars(pyo3_bar_type)

    async def _subscribe_funding_rates(self, command: SubscribeFundingRates) -> None:
        symbol = command.instrument_id.symbol.value

        if "PERPETUAL" not in symbol:
            self._log.warning(
                f"Funding rates subscription rejected for {command.instrument_id}: "
                "only available for perpetual instruments",
            )
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        interval = self._get_interval(command.params)
        interval_display = interval.name if interval else "100ms (default)"
        self._log.info(
            f"Subscribing to funding rates for {command.instrument_id} "
            f"(via perpetual channel, interval: {interval_display})",
        )
        await self._ws_client.subscribe_perpetual_interest_rates(pyo3_instrument_id, interval)

    async def _unsubscribe_instruments(self, command: UnsubscribeInstruments) -> None:
        kind = "any"
        currency = "any"

        if command.params:
            kind = command.params.get("kind", "any")
            currency = command.params.get("currency", "any")

        self._log.info(f"Unsubscribing from instrument state changes: {kind}.{currency}")
        await self._ws_client.unsubscribe_instrument_state(kind, currency)

    async def _unsubscribe_instrument(self, command: UnsubscribeInstrument) -> None:
        symbol = command.instrument_id.symbol.value

        if "PERPETUAL" in symbol:
            kind = "future"
        elif symbol.endswith(("-C", "-P")):
            kind = "option"
        elif "_" in symbol and "-" not in symbol:
            kind = "spot"
        else:
            kind = "future"

        parts = symbol.replace("_", "-").split("-")
        currency = parts[0] if parts else "any"

        self._log.info(
            f"Unsubscribing from instrument state for {command.instrument_id} "
            f"(channel: instrument.state.{kind}.{currency})",
        )
        await self._ws_client.unsubscribe_instrument_state(kind, currency)

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        interval = self._get_interval(command.params)

        depth = self._book_subscription_depths.pop(command.instrument_id, None)
        if depth is None and command.params:
            depth_str = command.params.get("depth")
            if depth_str:
                depth = int(depth_str)

        await self._ws_client.unsubscribe_book(pyo3_instrument_id, interval, depth)

    async def _unsubscribe_order_book_depth(self, command: UnsubscribeOrderBook) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        depth = 10  # Default for OrderBookDepth10
        group = "none"
        interval = self._get_interval(command.params)
        await self._ws_client.unsubscribe_book_grouped(pyo3_instrument_id, group, depth, interval)

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_quotes(pyo3_instrument_id)

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        interval = self._get_interval(command.params)
        await self._ws_client.unsubscribe_trades(pyo3_instrument_id, interval)

    async def _unsubscribe_mark_prices(self, command: UnsubscribeMarkPrices) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        interval = self._get_interval(command.params)
        interval_display = interval.name if interval else "100ms (default)"
        self._log.info(
            f"Unsubscribing from mark prices for {command.instrument_id} "
            f"(via ticker channel, interval: {interval_display})",
        )
        await self._ws_client.unsubscribe_ticker(pyo3_instrument_id, interval)

    async def _unsubscribe_index_prices(self, command: UnsubscribeIndexPrices) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        interval = self._get_interval(command.params)
        interval_display = interval.name if interval else "100ms (default)"
        self._log.info(
            f"Unsubscribing from index prices for {command.instrument_id} "
            f"(via ticker channel, interval: {interval_display})",
        )
        await self._ws_client.unsubscribe_ticker(pyo3_instrument_id, interval)

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(command.bar_type))
        await self._ws_client.unsubscribe_bars(pyo3_bar_type)

    async def _unsubscribe_funding_rates(self, command: UnsubscribeFundingRates) -> None:
        symbol = command.instrument_id.symbol.value

        if "PERPETUAL" not in symbol:
            self._log.warning(
                f"Funding rates unsubscription rejected for {command.instrument_id}: "
                "only available for perpetual instruments",
            )
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        interval = self._get_interval(command.params)
        interval_display = interval.name if interval else "100ms (default)"
        self._log.info(
            f"Unsubscribing from funding rates for {command.instrument_id} "
            f"(via perpetual channel, interval: {interval_display})",
        )
        await self._ws_client.unsubscribe_perpetual_interest_rates(pyo3_instrument_id, interval)

    async def _request_instrument(self, request: RequestInstrument) -> None:
        if request.start is not None:
            self._log.warning(
                f"Requesting instrument {request.instrument_id} with specified `start` which has no effect",
            )

        if request.end is not None:
            self._log.warning(
                f"Requesting instrument {request.instrument_id} with specified `end` which has no effect",
            )

        try:
            pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(request.instrument_id.value)
            pyo3_instrument = await self._http_client.request_instrument(pyo3_instrument_id)
            self._cache_instrument(pyo3_instrument)
            instrument = transform_instrument_from_pyo3(pyo3_instrument)
        except Exception as e:
            self._log.error(f"Failed to request instrument {request.instrument_id}: {e}")
            return

        self._handle_instrument(
            instrument,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    async def _fetch_instruments_for_currency(
        self,
        currency: nautilus_pyo3.DeribitCurrency,
        product_type: nautilus_pyo3.DeribitProductType | None = None,
    ) -> list[Instrument]:
        try:
            pyo3_instruments = await self._http_client.request_instruments(currency, product_type)
            instruments = []
            for pyo3_instrument in pyo3_instruments:
                self._cache_instrument(pyo3_instrument)
                instrument = transform_instrument_from_pyo3(pyo3_instrument)
                instruments.append(instrument)
            return instruments
        except Exception as e:
            product_type_str = f" product_type {product_type}" if product_type else ""
            self._log.error(f"Failed to fetch instruments for {currency}{product_type_str}: {e}")
            return []

    async def _request_instruments(self, request: RequestInstruments) -> None:
        if request.start is not None:
            self._log.warning(
                f"Requesting instruments for {request.venue} with specified `start` which has no effect",
            )

        if request.end is not None:
            self._log.warning(
                f"Requesting instruments for {request.venue} with specified `end` which has no effect",
            )

        all_instruments: list[Instrument] = []

        product_types = self._config.product_types

        if product_types:
            for product_type in product_types:
                instruments = await self._fetch_instruments_for_currency(
                    DeribitCurrency.ANY,
                    product_type,
                )
                all_instruments.extend(instruments)
        else:
            instruments = await self._fetch_instruments_for_currency(
                DeribitCurrency.ANY,
                nautilus_pyo3.DeribitProductType.FUTURE,
            )
            all_instruments.extend(instruments)

        self._handle_instruments(
            request.venue,
            all_instruments,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(request.instrument_id.value)

        try:
            pyo3_trades = await self._http_client.request_trades(
                instrument_id=pyo3_instrument_id,
                start=ensure_pydatetime_utc(request.start),
                end=ensure_pydatetime_utc(request.end),
                limit=request.limit or None,
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
        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(bar_type))

        try:
            pyo3_bars = await self._http_client.request_bars(
                bar_type=pyo3_bar_type,
                start=ensure_pydatetime_utc(request.start),
                end=ensure_pydatetime_utc(request.end),
                limit=request.limit or None,
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

    async def _request_order_book_snapshot(self, request: RequestOrderBookSnapshot) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(request.instrument_id.value)
        depth = request.limit or None
        try:
            pyo3_book = await self._http_client.request_book_snapshot(
                instrument_id=pyo3_instrument_id,
                depth=depth,
            )
        except Exception as e:
            self._log.exception(
                f"Failed to request book snapshot for {request.instrument_id}",
                e,
            )
            return

        instrument = self._cache.instrument(request.instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find instrument for {request.instrument_id}")
            return

        ts_event = pyo3_book.ts_last
        ts_init = self._clock.timestamp_ns()
        sequence = pyo3_book.sequence

        # Build OrderBookDeltas from pyo3 book data
        deltas: list[OrderBookDelta] = []
        deltas.append(OrderBookDelta.clear(request.instrument_id, sequence, ts_event, ts_init))

        bids = list(pyo3_book.bids())
        asks = list(pyo3_book.asks())

        for i, level in enumerate(bids):
            order = BookOrder(
                side=OrderSide.BUY,
                price=instrument.make_price(level.price.as_double()),
                size=instrument.make_qty(level.size()),
                order_id=i,
            )
            delta = OrderBookDelta(
                instrument_id=request.instrument_id,
                action=BookAction.ADD,
                order=order,
                flags=0,
                sequence=sequence,
                ts_event=ts_event,
                ts_init=ts_init,
            )
            deltas.append(delta)

        for i, level in enumerate(asks):
            order = BookOrder(
                side=OrderSide.SELL,
                price=instrument.make_price(level.price.as_double()),
                size=instrument.make_qty(level.size()),
                order_id=len(bids) + i,
            )
            delta = OrderBookDelta(
                instrument_id=request.instrument_id,
                action=BookAction.ADD,
                order=order,
                flags=0,
                sequence=sequence,
                ts_event=ts_event,
                ts_init=ts_init,
            )
            deltas.append(delta)

        # Set F_LAST flag on the actual last delta (after CLEAR + bids + asks)
        if deltas:
            last = deltas[-1]
            deltas[-1] = OrderBookDelta(
                instrument_id=last.instrument_id,
                action=last.action,
                order=last.order,
                flags=RecordFlag.F_LAST,
                sequence=last.sequence,
                ts_event=last.ts_event,
                ts_init=last.ts_init,
            )

        snapshot = OrderBookDeltas(instrument_id=request.instrument_id, deltas=deltas)

        data_type = DataType(
            OrderBookDeltas,
            metadata={"instrument_id": request.instrument_id},
        )
        self._handle_data_response(
            data_type=data_type,
            data=[snapshot],
            correlation_id=request.id,
            start=None,
            end=None,
            params=request.params,
        )

    def _handle_msg(self, msg: Any) -> None:
        try:
            if nautilus_pyo3.is_pycapsule(msg):
                # The capsule will fall out of scope at the end of this method,
                # and eventually be garbage collected. The contained pointer
                # to `Data` is still owned and managed by Rust.
                data = capsule_to_data(msg)
                self._handle_data(data)
            elif hasattr(msg, "__class__") and "Instrument" in msg.__class__.__name__:
                self._handle_instrument_update(msg)
            elif hasattr(msg, "__class__") and "FundingRateUpdate" in msg.__class__.__name__:
                self._handle_funding_rate_update(msg)
            else:
                self._log.error(f"Cannot handle message {msg}, not implemented")
        except Exception as e:
            self._log.exception("Error handling WebSocket message", e)

    def _cache_instrument(self, pyo3_instrument: Any) -> None:
        self._http_client.cache_instrument(pyo3_instrument)

        if self._ws_client is not None:
            self._ws_client.cache_instrument(pyo3_instrument)

    def _handle_instrument_update(self, pyo3_instrument: Any) -> None:
        self._cache_instrument(pyo3_instrument)

        instrument = transform_instrument_from_pyo3(pyo3_instrument)

        self._handle_data(instrument)

    def _handle_funding_rate_update(self, pyo3_funding_rate: Any) -> None:
        data = FundingRateUpdate.from_pyo3(pyo3_funding_rate)
        self._handle_data(data)
