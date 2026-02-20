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
Data client for the dYdX v4 decentralized crypto exchange.

This client uses the Rust-backed HTTP and WebSocket clients for market data.

"""

import asyncio

from nautilus_trader.adapters.dydx.config import DydxDataClientConfig
from nautilus_trader.adapters.dydx.constants import DYDX_VENUE
from nautilus_trader.adapters.dydx.providers import DydxInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.datetime import ensure_pydatetime_utc
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
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import FundingRateUpdate
from nautilus_trader.model.data import IndexPriceUpdate
from nautilus_trader.model.data import MarkPriceUpdate
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import book_type_to_str
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId


class DydxDataClient(LiveMarketDataClient):
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
    instrument_provider : DydxInstrumentProvider
        The instrument provider.
    config : DydxDataClientConfig
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
        instrument_provider: DydxInstrumentProvider,
        config: DydxDataClientConfig,
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

        self._instrument_provider: DydxInstrumentProvider = instrument_provider

        # Configuration
        self._config = config
        self._bars_timestamp_on_close = config.bars_timestamp_on_close
        self._log.info(f"{config.is_testnet=}", LogColor.BLUE)
        self._log.info(f"{config.bars_timestamp_on_close=}", LogColor.BLUE)
        self._log.info(f"{config.max_retries=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_initial_ms=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_max_ms=}", LogColor.BLUE)

        # HTTP API
        self._http_client = client

        # WebSocket API (using public client for market data)
        ws_url = config.base_url_ws or nautilus_pyo3.get_dydx_ws_url(config.is_testnet)  # type: ignore[attr-defined]
        self._ws_client = nautilus_pyo3.DydxWebSocketClient.new_public(  # type: ignore[attr-defined]
            url=ws_url,
            heartbeat=20,
        )
        self._ws_client.set_bars_timestamp_on_close(self._bars_timestamp_on_close)
        self._ws_client_futures: set[asyncio.Future] = set()

        # Quote synthesis state (quotes are derived from orderbook top-of-book)
        self._active_quote_subs: set[InstrumentId] = set()
        self._order_books: dict[InstrumentId, OrderBook] = {}
        self._last_quotes: dict[InstrumentId, QuoteTick] = {}

        # Subscription tracking (mirrors Rust DydxDataClient)
        self._active_delta_subs: set[InstrumentId] = set()
        self._active_trade_subs: set[InstrumentId] = set()
        self._active_mark_price_subs: set[InstrumentId] = set()
        self._active_index_price_subs: set[InstrumentId] = set()
        self._active_funding_rate_subs: set[InstrumentId] = set()

    @property
    def instrument_provider(self) -> DydxInstrumentProvider:
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
            self._log.debug("Disconnecting WebSocket")

            await self._ws_client.disconnect()

            self._log.debug(f"Disconnected from {self._ws_client.py_url}")

        # Cancel any pending futures
        await cancel_tasks_with_timeout(
            self._ws_client_futures,
            self._log,
            timeout_secs=DEFAULT_FUTURE_CANCELLATION_TIMEOUT,
        )

        self._ws_client_futures.clear()

        # Clear subscription state
        self._active_quote_subs.clear()
        self._active_delta_subs.clear()
        self._active_trade_subs.clear()
        self._active_mark_price_subs.clear()
        self._active_index_price_subs.clear()
        self._active_funding_rate_subs.clear()
        self._order_books.clear()
        self._last_quotes.clear()

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
            if nautilus_pyo3.is_pycapsule(capsule):
                data = capsule_to_data(capsule)

                # Synthesize quotes from orderbook deltas if we have active quote subscriptions
                if isinstance(data, OrderBookDeltas):
                    self._handle_orderbook_deltas(data)
                else:
                    self._handle_data(data)
                return

            if self._handle_market_data_update(capsule):
                return

            if isinstance(capsule, dict):
                self._handle_dict_message(capsule)
                return

            self._log.debug(f"Ignoring message of type {type(capsule).__name__}")
        except Exception as e:
            self._log.error(f"Error handling WebSocket message: {e}")

    def _handle_market_data_update(self, capsule: object) -> bool:
        if isinstance(capsule, nautilus_pyo3.MarkPriceUpdate):
            instrument_id = InstrumentId.from_str(capsule.instrument_id.value)
            if instrument_id in self._active_mark_price_subs:
                self._handle_data(MarkPriceUpdate.from_pyo3(capsule))
            return True

        if isinstance(capsule, nautilus_pyo3.IndexPriceUpdate):
            instrument_id = InstrumentId.from_str(capsule.instrument_id.value)
            if instrument_id in self._active_index_price_subs:
                self._handle_data(IndexPriceUpdate.from_pyo3(capsule))
            return True

        if isinstance(capsule, nautilus_pyo3.FundingRateUpdate):
            instrument_id = InstrumentId.from_str(capsule.instrument_id.value)
            if instrument_id in self._active_funding_rate_subs:
                self._handle_data(FundingRateUpdate.from_pyo3(capsule))
            return True

        return False

    def _handle_dict_message(self, capsule: dict) -> None:
        msg_type = capsule.get("type")
        if msg_type == "new_instrument_discovered":
            ticker = capsule.get("ticker")
            if ticker:
                self._log.info(
                    f"New instrument discovered via WebSocket: {ticker}",
                    LogColor.BLUE,
                )
                task = asyncio.create_task(self._fetch_new_instrument(ticker))
                self._ws_client_futures.add(task)
                task.add_done_callback(self._ws_client_futures.discard)

    async def _fetch_new_instrument(self, ticker: str) -> None:
        try:
            instrument = await self._http_client.fetch_instrument(ticker)
            if instrument is not None:
                self._ws_client.cache_instrument(instrument)
                self._instrument_provider.add(instrument)
                self._handle_data(instrument)
                self._log.info(
                    f"Fetched and cached new instrument: {ticker}",
                    LogColor.GREEN,
                )
            else:
                self._log.warning(f"New instrument {ticker} not found or inactive")
        except Exception as e:
            self._log.error(f"Failed to fetch new instrument {ticker}: {e}")

    def _handle_orderbook_deltas(self, deltas: OrderBookDeltas) -> None:
        instrument_id = deltas.instrument_id

        # Synthesize quote if this instrument has an active quote subscription
        if instrument_id in self._active_quote_subs:
            # Get or create local order book
            book = self._order_books.get(instrument_id)
            if book is None:
                book = OrderBook(instrument_id, book_type=BookType.L2_MBP)
                self._order_books[instrument_id] = book

            # Apply deltas to local order book
            book.apply(deltas)

            bid_price = book.best_bid_price()
            ask_price = book.best_ask_price()
            bid_size = book.best_bid_size()
            ask_size = book.best_ask_size()

            # Only synthesize quote if we have both bid and ask
            if (
                bid_price is not None
                and ask_price is not None
                and bid_size is not None
                and ask_size is not None
            ):
                # Deduplicate: only emit if top-of-book changed from last quote
                last_quote = self._last_quotes.get(instrument_id)
                if (
                    last_quote is None
                    or last_quote.bid_price != bid_price
                    or last_quote.ask_price != ask_price
                    or last_quote.bid_size != bid_size
                    or last_quote.ask_size != ask_size
                ):
                    quote = QuoteTick(
                        instrument_id=instrument_id,
                        bid_price=bid_price,
                        ask_price=ask_price,
                        bid_size=bid_size,
                        ask_size=ask_size,
                        ts_event=deltas.ts_event,
                        ts_init=deltas.ts_init,
                    )
                    self._last_quotes[instrument_id] = quote
                    self._handle_data(quote)

        # Only forward deltas if there's an active delta subscription
        if instrument_id in self._active_delta_subs:
            self._handle_data(deltas)

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

        # Track active subscription
        self._active_delta_subs.add(command.instrument_id)

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_orderbook(pyo3_instrument_id)

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        # dYdX doesn't have a dedicated quote tick channel
        # Quotes are synthesized from orderbook data (top-of-book)
        instrument_id = command.instrument_id

        # Track active quote subscription
        self._active_quote_subs.add(instrument_id)

        # Initialize local order book if needed
        if instrument_id not in self._order_books:
            self._order_books[instrument_id] = OrderBook(instrument_id, book_type=BookType.L2_MBP)

        # Subscribe to orderbook channel (quotes are derived from orderbook deltas)
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(instrument_id.value)
        await self._ws_client.subscribe_orderbook(pyo3_instrument_id)

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        # Track active subscription
        self._active_trade_subs.add(command.instrument_id)

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_trades(pyo3_instrument_id)

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        bar_type = command.bar_type

        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(bar_type))
        await self._ws_client.subscribe_bars(pyo3_bar_type)

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        # Remove from tracking
        self._active_delta_subs.discard(command.instrument_id)

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_orderbook(pyo3_instrument_id)

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        # Quotes are synthesized from orderbook data (top-of-book)
        instrument_id = command.instrument_id

        # Remove from active quote subscriptions
        self._active_quote_subs.discard(instrument_id)

        # Clean up state
        self._last_quotes.pop(instrument_id, None)

        # Unsubscribe from orderbook channel
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(instrument_id.value)
        await self._ws_client.unsubscribe_orderbook(pyo3_instrument_id)

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        # Remove from tracking
        self._active_trade_subs.discard(command.instrument_id)

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_trades(pyo3_instrument_id)

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        bar_type = command.bar_type

        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(bar_type))
        await self._ws_client.unsubscribe_bars(pyo3_bar_type)

    async def _unsubscribe_instruments(self, command: UnsubscribeInstruments) -> None:
        # Markets channel is always subscribed, no unsubscription needed
        pass

    async def _unsubscribe_instrument(self, command: UnsubscribeInstrument) -> None:
        # Markets channel is always subscribed, no per-instrument unsubscription
        pass

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
        # Track active subscription
        self._active_mark_price_subs.add(command.instrument_id)
        # dYdX provides mark prices through the markets channel (already subscribed)

    async def _unsubscribe_mark_prices(self, command: UnsubscribeMarkPrices) -> None:
        # Remove from tracking
        self._active_mark_price_subs.discard(command.instrument_id)
        # Mark prices are part of markets channel, no separate unsubscription

    async def _subscribe_index_prices(self, command: SubscribeIndexPrices) -> None:
        # Track active subscription
        self._active_index_price_subs.add(command.instrument_id)
        # dYdX provides index prices through the markets channel (already subscribed)

    async def _unsubscribe_index_prices(self, command: UnsubscribeIndexPrices) -> None:
        # Remove from tracking
        self._active_index_price_subs.discard(command.instrument_id)
        # Index prices are part of markets channel, no separate unsubscription

    async def _subscribe_funding_rates(self, command: SubscribeFundingRates) -> None:
        # Track active subscription
        self._active_funding_rate_subs.add(command.instrument_id)
        # dYdX provides funding rates through the markets channel (already subscribed)

    async def _unsubscribe_funding_rates(self, command: UnsubscribeFundingRates) -> None:
        # Remove from tracking
        self._active_funding_rate_subs.discard(command.instrument_id)
        # Funding rates are part of markets channel, no separate unsubscription

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

    async def _request_order_book_snapshot(self, request: RequestOrderBookSnapshot) -> None:
        try:
            pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                request.instrument_id.value,
            )

            pyo3_deltas = await self._http_client.request_orderbook_snapshot(
                instrument_id=pyo3_instrument_id,
            )

            deltas = OrderBookDeltas.from_pyo3(pyo3_deltas)

            self._handle_data_response(
                data_type=request.data_type,
                data=[deltas],
                correlation_id=request.id,
                start=None,
                end=None,
                params=request.params,
            )

        except Exception as e:
            self._log.error(
                f"Error requesting order book snapshot for {request.instrument_id}: {e}",
            )

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
            limit = request.limit if request.limit > 0 else None

            pyo3_trades = await self._http_client.request_trade_ticks(
                instrument_id=pyo3_instrument_id,
                start=ensure_pydatetime_utc(request.start),
                end=ensure_pydatetime_utc(request.end),
                limit=limit,
            )

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

    async def _request_bars(self, request: RequestBars) -> None:
        bar_type = request.bar_type
        limit = request.limit if request.limit > 0 else None

        self._log.info(
            f"Request {bar_type} bars from {request.start or 'start'} to {request.end or 'end'}",
        )

        try:
            pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(bar_type))
            bars = await self._http_client.request_bars(
                bar_type=pyo3_bar_type,
                start=ensure_pydatetime_utc(request.start),
                end=ensure_pydatetime_utc(request.end),
                limit=limit,
                timestamp_on_close=self._bars_timestamp_on_close,
            )
            bars = Bar.from_pyo3_list(bars)

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
