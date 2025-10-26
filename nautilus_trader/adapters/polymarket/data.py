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
from collections.abc import Coroutine
from typing import Any

import msgspec
from py_clob_client.client import ClobClient

from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_MAX_PRICE
from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_MIN_PRICE
from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_VENUE
from nautilus_trader.adapters.polymarket.common.deltas import compute_effective_deltas
from nautilus_trader.adapters.polymarket.common.enums import PolymarketOrderSide
from nautilus_trader.adapters.polymarket.common.parsing import update_instrument
from nautilus_trader.adapters.polymarket.common.symbol import get_polymarket_instrument_id
from nautilus_trader.adapters.polymarket.common.symbol import get_polymarket_token_id
from nautilus_trader.adapters.polymarket.config import PolymarketDataClientConfig
from nautilus_trader.adapters.polymarket.providers import PolymarketInstrumentProvider
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketBookLevel
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketBookSnapshot
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketQuote
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketQuotes
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketTickSizeChange
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketTrade
from nautilus_trader.adapters.polymarket.websocket.client import PolymarketWebSocketChannel
from nautilus_trader.adapters.polymarket.websocket.client import PolymarketWebSocketClient
from nautilus_trader.adapters.polymarket.websocket.types import MARKET_WS_MESSAGE
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.datetime import millis_to_nanos
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
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import BinaryOption


class PolymarketDataClient(LiveMarketDataClient):
    """
    Provides a data client for Polymarket, a decentralized predication market.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    http_client : py_clob_client.client.ClobClient
        The Polymarket HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : PolymarketInstrumentProvider
        The instrument provider.
    config : PolymarketDataClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        http_client: ClobClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: PolymarketInstrumentProvider,
        config: PolymarketDataClientConfig,
        name: str | None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or POLYMARKET_VENUE.value),
            venue=POLYMARKET_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        # Configuration
        self._config = config
        self._log.info(f"{config.signature_type=}", LogColor.BLUE)
        self._log.info(f"{config.funder=}", LogColor.BLUE)
        self._log.info(f"{config.ws_connection_initial_delay_secs=}", LogColor.BLUE)
        self._log.info(f"{config.ws_connection_delay_secs=}", LogColor.BLUE)
        self._log.info(f"{config.update_instruments_interval_mins=}", LogColor.BLUE)
        self._log.info(f"{config.compute_effective_deltas=}", LogColor.BLUE)

        # HTTP API
        self._http_client = http_client

        # WebSocket API
        self._ws_clients: list[PolymarketWebSocketClient] = []
        self._ws_client_pending_connection: PolymarketWebSocketClient | None = None

        self._decoder_market_msg = msgspec.json.Decoder(MARKET_WS_MESSAGE)

        # Tasks
        self._update_instruments_task: asyncio.Task | None = None
        self._delayed_ws_client_connection_task: asyncio.Task | None = None

        # Hot caches
        self._last_quotes: dict[InstrumentId, QuoteTick] = {}
        self._local_books: dict[InstrumentId, OrderBook] = {}

    async def _connect(self) -> None:
        self._log.info("Initializing instruments...")
        await self._instrument_provider.initialize()
        self._send_all_instruments_to_data_engine()

        if self._config.update_instruments_interval_mins:
            self._update_instruments_task = self.create_task(
                self._update_instruments(self._config.update_instruments_interval_mins),
            )

    async def _disconnect(self) -> None:
        if self._update_instruments_task:
            self._log.debug("Canceling task 'update_instruments'")
            self._update_instruments_task.cancel()
            self._update_instruments_task = None

        if self._delayed_ws_client_connection_task:
            self._log.debug("Canceling task 'delayed_ws_client_connection'")
            self._delayed_ws_client_connection_task.cancel()
            self._delayed_ws_client_connection_task = None

        # Shutdown websockets
        tasks: set[Coroutine[Any, Any, None]] = set()

        for ws_client in self._ws_clients:
            if ws_client.is_connected():
                tasks.add(ws_client.disconnect())

        if tasks:
            await asyncio.gather(*tasks)

        self._cleanup_expired_books()

    def _create_websocket_client(self) -> PolymarketWebSocketClient:
        self._log.info("Creating new PolymarketWebSocketClient", LogColor.MAGENTA)
        return PolymarketWebSocketClient(
            self._clock,
            base_url=self._config.base_url_ws,
            channel=PolymarketWebSocketChannel.MARKET,
            handler=self._handle_raw_ws_message,
            handler_reconnect=None,
            loop=self._loop,
        )

    def _create_local_book(self, instrument_id: InstrumentId) -> OrderBook:
        local_book = OrderBook(instrument_id, book_type=BookType.L2_MBP)
        self._local_books[instrument_id] = local_book
        return local_book

    def _cleanup_expired_books(self) -> None:
        now_ns = self._clock.timestamp_ns()
        expired_instruments = []

        for instrument_id in list(self._local_books.keys()):
            instrument = self._cache.instrument(instrument_id)
            if instrument and instrument.expiration_ns < now_ns:
                expired_instruments.append(instrument_id)

        if expired_instruments:
            for instrument_id in expired_instruments:
                self._local_books.pop(instrument_id, None)
                self._last_quotes.pop(instrument_id, None)
            self._log.info(f"Cleaned up {len(expired_instruments)} expired book(s)")

    def _send_all_instruments_to_data_engine(self) -> None:
        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)

        for currency in self._instrument_provider.currencies().values():
            self._cache.add_currency(currency)

    async def _update_instruments(self, interval_mins: int) -> None:
        try:
            while True:
                self._log.debug(
                    f"Scheduled task 'update_instruments' to run in {interval_mins} minutes",
                )
                await asyncio.sleep(interval_mins * 60)
                await self._instrument_provider.initialize(reload=True)
                self._send_all_instruments_to_data_engine()
        except asyncio.CancelledError:
            self._log.debug("Canceled task 'update_instruments'")

    async def _delayed_ws_client_connection(
        self,
        ws_client: PolymarketWebSocketClient,
        delay_secs: float,
    ) -> None:
        try:
            self._log.info(f"Delaying websocket connections start for {delay_secs}s...")

            await asyncio.sleep(delay_secs)
            self._ws_clients.append(ws_client)
            await ws_client.connect()
        finally:
            self._ws_client_pending_connection = None
            self._delayed_ws_client_connection_task = None

    async def _subscribe_asset_book(self, instrument_id):
        create_connect_task = False
        # Polymarket only supports 500 subscriptions per client
        if (
            self._ws_client_pending_connection is None
            or len(self._ws_client_pending_connection.asset_subscriptions()) >= 500
        ):
            self._ws_client_pending_connection = self._create_websocket_client()
            create_connect_task = True

        token_id = get_polymarket_token_id(instrument_id)
        if token_id in self._ws_client_pending_connection.asset_subscriptions():
            return  # Already subscribed

        self._ws_client_pending_connection.subscribe_book(token_id)

        if create_connect_task:
            # Cancel previous delayed connection task to prevent race condition
            # where old task's finally block nulls out the new pending client
            if (
                self._delayed_ws_client_connection_task
                and not self._delayed_ws_client_connection_task.done()
            ):
                self._delayed_ws_client_connection_task.cancel()

            self._delayed_ws_client_connection_task = self.create_task(
                self._delayed_ws_client_connection(
                    self._ws_client_pending_connection,
                    (
                        self._config.ws_connection_delay_secs
                        if self._ws_clients
                        else self._config.ws_connection_initial_delay_secs
                    ),
                ),
                log_msg="Delayed start PolymarketWebSocketClient connection",
                success_msg="Finished delaying start of PolymarketWebSocketClient connection",
            )

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        if command.book_type == BookType.L3_MBO:
            self._log.error(
                "Cannot subscribe to order book deltas: "
                "L3_MBO data is not published by Polymarket. "
                "Valid book types are L1_MBP, L2_MBP",
            )
            return

        if command.instrument_id not in self._local_books:
            self._create_local_book(command.instrument_id)

        await self._subscribe_asset_book(command.instrument_id)

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        if command.instrument_id not in self._local_books:
            self._create_local_book(command.instrument_id)

        await self._subscribe_asset_book(command.instrument_id)

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        await self._subscribe_asset_book(command.instrument_id)

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        self._log.error(
            f"Cannot subscribe to {command.bar_type} bars: not implemented for Polymarket",
        )

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        self._log.error(
            f"Cannot unsubscribe from {command.instrument_id} order book deltas: unsubscribing not supported by Polymarket",
        )

    async def _unsubscribe_order_book_snapshots(self, command: UnsubscribeOrderBook) -> None:
        self._log.error(
            f"Cannot unsubscribe from {command.instrument_id} order book snapshots: unsubscribing not supported by Polymarket",
        )

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        self._log.error(
            f"Cannot unsubscribe from {command.instrument_id} quotes: unsubscribing not supported by Polymarket",
        )

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        self._log.error(
            f"Cannot unsubscribe from {command.instrument_id} trades: unsubscribing not supported by Polymarket",
        )

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        self._log.error(
            f"Cannot unsubscribe from {command.bar_type} bars: not implemented for Polymarket",
        )

    async def _request_instrument(self, request: RequestInstrument) -> None:
        if request.start is not None:
            self._log.warning(
                f"Requesting instrument {request.instrument_id} with specified `start` which has no effect",
            )

        if request.end is not None:
            self._log.warning(
                f"Requesting instrument {request.instrument_id} with specified `end` which has no effect",
            )

        instrument: BinaryOption | None = self._instrument_provider.find(request.instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find instrument for {request.instrument_id}")
            return

        self._handle_instrument(instrument, request.id, request.start, request.end, request.params)

    async def _request_instruments(self, request: RequestInstruments) -> None:
        if request.start is not None:
            self._log.warning(
                f"Requesting instruments for {request.venue} with specified `start` which has no effect",
            )

        if request.end is not None:
            self._log.warning(
                f"Requesting instruments for {request.venue} with specified `end` which has no effect",
            )

        all_instruments = self._instrument_provider.get_all()
        target_instruments = []
        for instrument in all_instruments.values():
            if instrument.venue == request.venue:
                target_instruments.append(instrument)

        self._handle_instruments(
            request.venue,
            target_instruments,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    async def _request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        self._log.error("Cannot request historical quotes: not published by Polymarket")

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        self._log.error("Cannot request historical trades: not published by Polymarket")

    async def _request_bars(self, request: RequestBars) -> None:
        self._log.error("Cannot request historical bars: not published by Polymarket")

    def _handle_raw_ws_message(self, raw: bytes) -> None:
        # Uncomment for development
        # self._log.info(str(raw), LogColor.MAGENTA)
        try:
            msg = self._decoder_market_msg.decode(raw)

            if isinstance(msg, list):
                for item in msg:
                    self._handle_ws_message(item)
            else:
                self._handle_ws_message(msg)
        except Exception as e:
            self._log.exception(f"Failed to parse websocket message: {raw.decode()} with error", e)

    def _handle_ws_message(self, msg: Any) -> None:
        if isinstance(msg, PolymarketQuotes):
            self._handle_quotes(ws_message=msg)
        elif isinstance(msg, PolymarketBookSnapshot):
            instrument_id = get_polymarket_instrument_id(msg.market, msg.asset_id)
            instrument = self._cache.instrument(instrument_id)
            if instrument is None:
                self._log.error(f"Cannot find instrument for {instrument_id}")
                return
            self._handle_book_snapshot(instrument=instrument, ws_message=msg)
        elif isinstance(msg, PolymarketTrade):
            instrument_id = get_polymarket_instrument_id(msg.market, msg.asset_id)
            instrument = self._cache.instrument(instrument_id)
            if instrument is None:
                self._log.error(f"Cannot find instrument for {instrument_id}")
                return
            self._handle_trade(instrument=instrument, ws_message=msg)
        elif isinstance(msg, PolymarketTickSizeChange):
            instrument_id = get_polymarket_instrument_id(msg.market, msg.asset_id)
            instrument = self._cache.instrument(instrument_id)
            if instrument is None:
                self._log.error(f"Cannot find instrument for {instrument_id}")
                return
            self._handle_instrument_update(instrument=instrument, ws_message=msg)
        else:
            self._log.error(f"Unknown websocket message topic: {msg}")

    def _handle_book_snapshot(
        self,
        instrument: BinaryOption,
        ws_message: PolymarketBookSnapshot,
    ) -> None:
        now_ns = self._clock.timestamp_ns()
        deltas = ws_message.parse_to_snapshot(instrument=instrument, ts_init=now_ns)

        if deltas is None:
            # Skip empty snapshots (can occur near market resolution)
            return

        self._handle_deltas(instrument, deltas)

        if instrument.id in self.subscribed_quote_ticks():
            quote = ws_message.parse_to_quote(
                instrument=instrument,
                ts_init=now_ns,
                drop_quotes_missing_side=self._config.drop_quotes_missing_side,
            )
            if quote is None:
                self._log.warning(
                    f"Dropping QuoteTick for {instrument.id}: missing bid or ask prices in snapshot",
                )
                return
            self._last_quotes[instrument.id] = quote
            self._handle_data(quote)

    def _handle_deltas(self, instrument: BinaryOption, deltas: OrderBookDeltas) -> None:
        # Always maintain local book for quote generation
        book_old = self._local_books.get(instrument.id)
        book_new = OrderBook(instrument.id, book_type=BookType.L2_MBP)
        book_new.apply_deltas(deltas)
        self._local_books[instrument.id] = book_new

        if self._config.compute_effective_deltas and book_old is not None:
            # Compute effective deltas (reduce snapshot based on old and new book states),
            # prioritizing a smaller data footprint over computational efficiency.
            t0 = self._clock.timestamp_ns()
            deltas = compute_effective_deltas(book_old, book_new, instrument)

            interval_ms = (self._clock.timestamp_ns() - t0) / 1_000_000
            self._log.debug(f"Computed effective deltas in {interval_ms:.3f}ms")
            # self._log.warning(book_new.pprint())  # Uncomment for development

        # Check if any effective deltas remain
        if deltas:
            self._handle_data(deltas)

    def _handle_quotes(
        self,
        ws_message: PolymarketQuotes,
    ) -> None:
        for price_change in ws_message.price_changes:
            instrument_id = get_polymarket_instrument_id(ws_message.market, price_change.asset_id)
            instrument = self._cache.instrument(instrument_id)
            if instrument is None:
                self._log.error(f"Cannot find instrument for {instrument_id}")
                continue

            self._handle_quote(
                instrument=instrument,
                ws_message=ws_message,
                price_change=price_change,
            )

    def _handle_quote(
        self,
        instrument: BinaryOption,
        ws_message: PolymarketQuotes,
        price_change: PolymarketQuote,
    ) -> None:
        now_ns = self._clock.timestamp_ns()

        order = BookOrder(
            side=OrderSide.BUY if price_change.side == PolymarketOrderSide.BUY else OrderSide.SELL,
            price=instrument.make_price(float(price_change.price)),
            size=instrument.make_qty(float(price_change.size)),
            order_id=0,
        )
        delta = OrderBookDelta(
            instrument_id=instrument.id,
            action=BookAction.UPDATE if order.size > 0 else BookAction.DELETE,
            order=order,
            flags=RecordFlag.F_LAST,
            sequence=0,
            ts_event=millis_to_nanos(float(ws_message.timestamp)),
            ts_init=now_ns,
        )
        deltas = OrderBookDeltas(instrument.id, [delta])

        # Check if local book exists, create if needed
        if instrument.id not in self._local_books:
            # Skip this quote if we're not subscribed to anything for this instrument
            if (
                instrument.id not in self.subscribed_quote_ticks()
                and instrument.id not in self.subscribed_order_book_deltas()
            ):
                return
            self._create_local_book(instrument.id)

        local_book = self._local_books[instrument.id]
        local_book.apply(deltas)

        self._handle_data(deltas)

        if instrument.id in self.subscribed_quote_ticks():
            bid_price = local_book.best_bid_price()
            ask_price = local_book.best_ask_price()
            bid_size = local_book.best_bid_size()
            ask_size = local_book.best_ask_size()

            # Handle missing bid/ask prices (can occur near market resolution)
            if bid_price is None or ask_price is None:
                if self._config.drop_quotes_missing_side:
                    self._log.warning(
                        f"Dropping QuoteTick for {instrument.id}: "
                        f"bid_price={bid_price}, ask_price={ask_price}",
                    )
                    return
                else:
                    # Use boundary prices with zero volume for missing sides
                    # POLYMARKET_MIN_PRICE = 0.001, POLYMARKET_MAX_PRICE = 0.999
                    if bid_price is None:
                        bid_price = instrument.make_price(POLYMARKET_MIN_PRICE)
                        bid_size = instrument.make_qty(0.0)
                    if ask_price is None:
                        ask_price = instrument.make_price(POLYMARKET_MAX_PRICE)
                        ask_size = instrument.make_qty(0.0)

            quote = QuoteTick(
                instrument_id=instrument.id,
                bid_price=bid_price,
                ask_price=ask_price,
                bid_size=bid_size,
                ask_size=ask_size,
                ts_event=millis_to_nanos(float(ws_message.timestamp)),
                ts_init=self._clock.timestamp_ns(),
            )

            last_quote = self._last_quotes.get(instrument.id)

            if last_quote is not None:
                if (
                    quote.bid_price == last_quote.bid_price
                    and quote.ask_price == last_quote.ask_price
                    and quote.bid_size == last_quote.bid_size
                    and quote.ask_size == last_quote.ask_size
                ):
                    return  # No top-of-book change

            self._last_quotes[instrument.id] = quote
            self._handle_data(quote)

    def _handle_trade(
        self,
        instrument: BinaryOption,
        ws_message: PolymarketTrade,
    ) -> None:
        now_ns = self._clock.timestamp_ns()
        trade = ws_message.parse_to_trade_tick(instrument=instrument, ts_init=now_ns)
        self._handle_data(trade)

    def _handle_instrument_update(
        self,
        instrument: BinaryOption,
        ws_message: PolymarketTickSizeChange,
    ) -> None:
        now_ns = self._clock.timestamp_ns()

        old_book = self._local_books.get(instrument.id)
        old_quote = self._last_quotes.get(instrument.id)

        instrument = update_instrument(instrument, change=ws_message, ts_init=now_ns)

        # Update local sources immediately so subsequent quotes use the correct precision
        self._instrument_provider.add(instrument)
        self._cache.add_instrument(instrument)

        self._log.warning(f"Instrument tick size changed: {instrument}")
        self._handle_data(instrument)

        if old_book is not None:
            self._reset_local_book_after_tick_size_change(
                instrument=instrument,
                change=ws_message,
                old_book=old_book,
                old_quote=old_quote,
                ts_init=now_ns,
            )

    def _reset_local_book_after_tick_size_change(
        self,
        instrument: BinaryOption,
        change: PolymarketTickSizeChange,
        old_book: OrderBook,
        old_quote: QuoteTick | None,
        ts_init: int,
    ) -> None:
        snapshot = self._build_snapshot_from_book(
            instrument=instrument,
            change=change,
            book=old_book,
        )

        deltas = snapshot.parse_to_snapshot(instrument=instrument, ts_init=ts_init)

        if deltas is None:
            self._local_books.pop(instrument.id, None)
            self._last_quotes.pop(instrument.id, None)
            return

        new_book = OrderBook(instrument.id, book_type=BookType.L2_MBP)
        new_book.apply_deltas(deltas)
        self._local_books[instrument.id] = new_book

        if self._config.compute_effective_deltas:
            effective = compute_effective_deltas(old_book, new_book, instrument)
            if effective:
                self._handle_data(effective)
        else:
            self._handle_data(deltas)

        if instrument.id in self.subscribed_quote_ticks():
            quote = snapshot.parse_to_quote(
                instrument=instrument,
                ts_init=ts_init,
                drop_quotes_missing_side=self._config.drop_quotes_missing_side,
            )
            if quote is not None:
                self._last_quotes[instrument.id] = quote
                self._handle_data(quote)
            elif old_quote is None:
                self._last_quotes.pop(instrument.id, None)

    def _build_snapshot_from_book(
        self,
        instrument: BinaryOption,
        change: PolymarketTickSizeChange,
        book: OrderBook,
    ) -> PolymarketBookSnapshot:
        bids_levels = [
            PolymarketBookLevel(
                price=str(instrument.make_price(float(level.price))),
                size=str(instrument.make_qty(level.size())),
            )
            for level in reversed(book.bids())
        ]

        asks_levels = [
            PolymarketBookLevel(
                price=str(instrument.make_price(float(level.price))),
                size=str(instrument.make_qty(level.size())),
            )
            for level in reversed(book.asks())
        ]

        return PolymarketBookSnapshot(
            market=change.market,
            asset_id=change.asset_id,
            bids=bids_levels,
            asks=asks_levels,
            timestamp=change.timestamp,
        )
