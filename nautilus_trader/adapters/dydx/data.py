# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
Provide a data client for the dYdX decentralized cypto exchange.
"""

import asyncio
from typing import TYPE_CHECKING, Any

import msgspec
import pandas as pd

from nautilus_trader.adapters.dydx.common.constants import DYDX_VENUE
from nautilus_trader.adapters.dydx.common.enums import DYDXEnumParser
from nautilus_trader.adapters.dydx.common.parsing import get_interval_from_bar_type
from nautilus_trader.adapters.dydx.common.symbol import DYDXSymbol
from nautilus_trader.adapters.dydx.config import DYDXDataClientConfig
from nautilus_trader.adapters.dydx.http.client import DYDXHttpClient
from nautilus_trader.adapters.dydx.http.market import DYDXMarketHttpAPI
from nautilus_trader.adapters.dydx.providers import DYDXInstrumentProvider
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsCandlesChannelData
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsCandlesSubscribedData
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsMarketChannelData
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsMarketSubscribedData
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsMessageGeneral
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsOrderbookBatchedData
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsOrderbookChannelData
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsOrderbookSnapshotChannelData
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsTradeChannelData
from nautilus_trader.adapters.dydx.websocket.client import DYDXWebsocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId


if TYPE_CHECKING:
    from collections.abc import Callable


class DYDXDataClient(LiveMarketDataClient):
    """
    Provide a data client for the dYdX decentralized cypto exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : DYDXHttpClient
        The dYdX HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : DYDXInstrumentProvider
        The instrument provider.
    ws_base_url: str
        The product base url for the WebSocket client.
    config : DYDXDataClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: DYDXHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: DYDXInstrumentProvider,
        ws_base_url: str,
        config: DYDXDataClientConfig,
        name: str | None,
    ) -> None:
        """
        Provide a data client for the dYdX decentralized cypto exchange.
        """
        super().__init__(
            loop=loop,
            client_id=ClientId(name or DYDX_VENUE.value),
            venue=DYDX_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        self._enum_parser = DYDXEnumParser()

        # Decoders
        self._decoder_ws_msg_general = msgspec.json.Decoder(DYDXWsMessageGeneral)
        self._decoder_ws_orderbook = msgspec.json.Decoder(DYDXWsOrderbookChannelData)
        self._decoder_ws_orderbook_batched = msgspec.json.Decoder(DYDXWsOrderbookBatchedData)
        self._decoder_ws_orderbook_snapshot = msgspec.json.Decoder(
            DYDXWsOrderbookSnapshotChannelData,
        )
        self._decoder_ws_trade = msgspec.json.Decoder(DYDXWsTradeChannelData)
        self._decoder_ws_kline = msgspec.json.Decoder(DYDXWsCandlesChannelData)
        self._decoder_ws_kline_subscribed = msgspec.json.Decoder(DYDXWsCandlesSubscribedData)
        self._decoder_ws_instruments = msgspec.json.Decoder(DYDXWsMarketChannelData)
        self._decoder_ws_instruments_subscribed = msgspec.json.Decoder(DYDXWsMarketSubscribedData)
        self._ws_client = DYDXWebsocketClient(
            clock=clock,
            handler=self._handle_ws_message,
            handler_reconnect=None,
            base_url=ws_base_url,
            loop=loop,
        )

        # HTTP API
        self._http_market = DYDXMarketHttpAPI(client=client, clock=clock)

        self._books: dict[InstrumentId, OrderBook] = {}
        self._topic_bar_type: dict[str, BarType] = {}

        self._update_instrument_interval: int = 60 * 60  # Once per hour (hardcode)
        self._update_orderbook_interval: int = 60  # Once every 60 seconds (hardcode)
        self._update_instruments_task: asyncio.Task | None = None
        self._resubscribe_orderbook_task: asyncio.Task | None = None
        self._last_quotes: dict[InstrumentId, QuoteTick] = {}
        self._orderbook_subscriptions: set[str] = set()
        self._resubscribe_orderbook_lock = asyncio.Lock()

        # Hot caches
        self._bars: dict[BarType, Bar] = {}

    async def _connect(self) -> None:
        self._log.info("Initializing instruments...")
        await self._instrument_provider.initialize()

        self._send_all_instruments_to_data_engine()
        self._update_instruments_task = self.create_task(self._update_instruments())
        self._resubscribe_orderbook_task = self.create_task(
            self._resubscribe_orderbooks_on_interval(),
        )

        self._log.info("Initializing websocket connection")
        await self._ws_client.connect()

        self._log.info("Data client connected")

    async def _disconnect(self) -> None:
        if self._update_instruments_task:
            self._log.debug("Cancelling `update_instruments` task")
            self._update_instruments_task.cancel()
            self._update_instruments_task = None

        if self._resubscribe_orderbook_task:
            self._log.debug("Cancelling `resubscribe_orderbook` task")
            self._resubscribe_orderbook_task.cancel()
            self._resubscribe_orderbook_task = None

        await self._ws_client.disconnect()

        self._log.info("Data client disconnected")

    async def _update_instruments(self) -> None:
        try:
            while True:
                self._log.debug(
                    f"Scheduled `update_instruments` to run in {self._update_instrument_interval}s",
                )
                await asyncio.sleep(self._update_instrument_interval)
                await self._instrument_provider.load_all_async()
                self._send_all_instruments_to_data_engine()
        except asyncio.CancelledError:
            self._log.debug("Canceled `update_instruments` task")

    async def _resubscribe_orderbooks_on_interval(self) -> None:
        """
        Resubscribe to the orderbook on a fixed interval `update_orderbook_interval` to
        ensure it does not become outdated.
        """
        try:
            while True:
                self._log.debug(
                    f"Scheduled `resubscribe_order_book` to run in {self._update_orderbook_interval}s",
                )
                await asyncio.sleep(self._update_orderbook_interval)
                await self._resubscribe_orderbooks()
        except asyncio.CancelledError:
            self._log.debug("Canceled `resubscribe_orderbook` task")

    async def _resubscribe_orderbooks(self) -> None:
        """
        Resubscribe to the orderbook.
        """
        async with self._resubscribe_orderbook_lock:
            for symbol in self._orderbook_subscriptions:
                await self._ws_client.unsubscribe_order_book(symbol, remove_subscription=False)
                await self._ws_client.subscribe_order_book(
                    symbol,
                    bypass_subscription_validation=True,
                )

    def _send_all_instruments_to_data_engine(self) -> None:
        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)

        for currency in self._instrument_provider.currencies().values():
            self._cache.add_currency(currency)

    def _handle_ws_message(self, raw: bytes) -> None:
        callbacks: dict[tuple[str | None, str | None], Callable[[bytes], None]] = {
            ("v4_orderbook", "channel_data"): self._handle_orderbook,
            ("v4_orderbook", "subscribed"): self._handle_orderbook_snapshot,
            ("v4_orderbook", "channel_batch_data"): self._handle_orderbook_batched,
            ("v4_trades", "channel_data"): self._handle_trade,
            ("v4_trades", "subscribed"): self._handle_trade_subscribed,
            ("v4_candles", "channel_data"): self._handle_kline,
            ("v4_candles", "subscribed"): self._handle_kline_subscribed,
            ("v4_markets", "channel_data"): self._handle_markets,
            ("v4_markets", "subscribed"): self._handle_markets_subscribed,
        }
        try:
            ws_message = self._decoder_ws_msg_general.decode(raw)
            key = (ws_message.channel, ws_message.type)

            if key in callbacks:
                callbacks[key](raw)
                return

            if ws_message.type == "unsubscribed":
                self._log.debug(
                    f"Unsubscribed from channel {ws_message.channel} for {ws_message.id}",
                )

                if ws_message.channel == "v4_candles":
                    self._handle_kline_unsubscribed(ws_message)
            elif ws_message.type == "connected":
                self._log.info("Websocket connected")
            elif ws_message.type == "error":
                self._log.error(f"Websocket error: {ws_message.message}")
            else:
                self._log.error(
                    f"Unknown message `{ws_message.channel}` `{ws_message.type}`: {raw.decode()}",
                )
        except Exception as e:
            self._log.error(f"Failed to parse websocket message: {raw.decode()} with error {e}")

    def _handle_trade(self, raw: bytes) -> None:
        try:
            msg: DYDXWsTradeChannelData = self._decoder_ws_trade.decode(raw)
            symbol = msg.id
            instrument_id: InstrumentId = self._get_cached_instrument_id(symbol)

            instrument = self._cache.instrument(instrument_id)

            if instrument is None:
                self._log.error(f"Cannot parse trade data: no instrument for {instrument_id}")
                return

            for tick_msg in msg.contents.trades:
                trade_tick = tick_msg.parse_to_trade_tick(
                    instrument_id,
                    price_precision=instrument.price_precision,
                    size_precision=instrument.size_precision,
                    ts_init=self._clock.timestamp_ns(),
                )
                self._handle_data(trade_tick)

        except Exception as e:
            self._log.error(f"Failed to parse trade tick: {raw.decode()} with error {e}")

    def _handle_trade_subscribed(self, raw: bytes) -> None:
        # Do not send the historical trade ticks to the DataEngine for the initial subscribed message.
        pass

    def _handle_orderbook(self, raw: bytes) -> None:
        try:
            msg: DYDXWsOrderbookChannelData = self._decoder_ws_orderbook.decode(raw)

            symbol = msg.id
            instrument_id: InstrumentId = self._get_cached_instrument_id(symbol)

            instrument = self._cache.instrument(instrument_id)

            if instrument is None:
                self._log.error(f"Cannot parse orderbook data: no instrument for {instrument_id}")
                return

            deltas = msg.parse_to_deltas(
                instrument_id=instrument_id,
                price_precision=instrument.price_precision,
                size_precision=instrument.size_precision,
                ts_event=self._clock.timestamp_ns(),
                ts_init=self._clock.timestamp_ns(),
            )

            self._handle_deltas(instrument_id=instrument_id, deltas=deltas)

        except Exception as e:
            self._log.error(f"Failed to parse orderbook: {raw.decode()} with error {e}")

    def _handle_orderbook_batched(self, raw: bytes) -> None:
        try:
            msg = self._decoder_ws_orderbook_batched.decode(raw)

            symbol = msg.id
            instrument_id: InstrumentId = self._get_cached_instrument_id(symbol)

            instrument = self._cache.instrument(instrument_id)

            if instrument is None:
                self._log.error(f"Cannot parse orderbook data: no instrument for {instrument_id}")
                return

            deltas = msg.parse_to_deltas(
                instrument_id=instrument_id,
                price_precision=instrument.price_precision,
                size_precision=instrument.size_precision,
                ts_event=self._clock.timestamp_ns(),
                ts_init=self._clock.timestamp_ns(),
            )

            self._handle_deltas(instrument_id=instrument_id, deltas=deltas)

        except Exception as e:
            self._log.error(f"Failed to parse orderbook: {raw.decode()} with error {e}")

    def _handle_orderbook_snapshot(self, raw: bytes) -> None:
        try:
            msg: DYDXWsOrderbookSnapshotChannelData = self._decoder_ws_orderbook_snapshot.decode(
                raw,
            )

            symbol = msg.id
            instrument_id: InstrumentId = self._get_cached_instrument_id(symbol)

            instrument = self._cache.instrument(instrument_id)

            if instrument is None:
                self._log.error(
                    f"Cannot parse orderbook snapshot: no instrument for {instrument_id}",
                )
                return

            deltas = msg.parse_to_snapshot(
                instrument_id=instrument_id,
                price_precision=instrument.price_precision,
                size_precision=instrument.size_precision,
                ts_event=self._clock.timestamp_ns(),
                ts_init=self._clock.timestamp_ns(),
            )

            self._handle_deltas(instrument_id=instrument_id, deltas=deltas)

        except Exception as e:
            self._log.error(f"Failed to parse orderbook snapshot: {raw.decode()} with error {e}")

    def _handle_deltas(self, instrument_id: InstrumentId, deltas: OrderBookDeltas) -> None:
        self._handle_data(deltas)

        if instrument_id in self._books:
            book = self._books[instrument_id]
            book.apply_deltas(deltas)

            last_quote = self._last_quotes.get(instrument_id)

            bid_price = book.best_bid_price()
            ask_price = book.best_ask_price()
            bid_size = book.best_bid_size()
            ask_size = book.best_ask_size()

            if bid_price is None and last_quote is not None:
                bid_price = last_quote.bid_price

            if ask_price is None and last_quote is not None:
                ask_price = last_quote.ask_price

            if bid_size is None and last_quote is not None:
                bid_size = last_quote.bid_size

            if ask_size is None and last_quote is not None:
                ask_size = last_quote.ask_size

            quote = QuoteTick(
                instrument_id=instrument_id,
                bid_price=bid_price,
                ask_price=ask_price,
                bid_size=bid_size,
                ask_size=ask_size,
                ts_event=deltas.ts_event,
                ts_init=deltas.ts_init,
            )

            if (
                last_quote is None
                or last_quote.bid_price != quote.bid_price
                or last_quote.ask_price != quote.ask_price
                or last_quote.bid_size != quote.bid_size
                or last_quote.ask_size != quote.ask_size
            ):
                self._handle_data(quote)
                self._last_quotes[instrument_id] = quote

    def _handle_kline(self, raw: bytes) -> None:
        try:
            msg: DYDXWsCandlesChannelData = self._decoder_ws_kline.decode(raw)

            symbol = msg.contents.ticker
            instrument_id: InstrumentId = self._get_cached_instrument_id(symbol)

            instrument = self._cache.instrument(instrument_id)

            if instrument is None:
                self._log.error(f"Cannot parse kline data: no instrument for {instrument_id}")
                return

            bar_type = self._topic_bar_type.get(msg.id)

            if bar_type is None:
                self._log.error(f"Cannot parse kline data: no bar type for {instrument_id}")
                return

            parsed_bar = msg.contents.parse_to_bar(
                bar_type=bar_type,
                price_precision=instrument.price_precision,
                size_precision=instrument.size_precision,
                ts_init=self._clock.timestamp_ns(),
            )

            # Klines which are not closed are pushed regularly.
            # Maintain a cache of bars, and only send closed bars to the DataEngine.
            last_bar = self._bars.get(bar_type)

            if last_bar is not None and last_bar.ts_event < parsed_bar.ts_event:
                self._handle_data(last_bar)

            self._bars[bar_type] = parsed_bar

        except Exception as e:
            self._log.error(f"Failed to parse kline data: {raw.decode()} with error {e}")

    def _handle_kline_subscribed(self, raw: bytes) -> None:
        # Do not send the historical bars to the DataEngine for the initial subscribed message.
        pass

    def _handle_kline_unsubscribed(self, msg: DYDXWsMessageGeneral) -> None:
        if msg.id is not None:
            self._topic_bar_type.pop(msg.id, None)

    def _handle_markets(self, raw: bytes) -> None:
        try:
            msg: DYDXWsMarketChannelData = self._decoder_ws_instruments.decode(raw)

            self._log.debug(f"{msg}")

        except Exception as e:
            self._log.error(f"Failed to parse market data: {raw.decode()} with error {e}")

    def _handle_markets_subscribed(self, raw: bytes) -> None:
        try:
            msg: DYDXWsMarketSubscribedData = self._decoder_ws_instruments_subscribed.decode(raw)

            self._log.debug(f"{msg}")

        except Exception as e:
            self._log.error(f"Failed to parse market channel data: {raw.decode()} with error {e}")

    async def _subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        dydx_symbol = DYDXSymbol(instrument_id.symbol.value)
        await self._ws_client.subscribe_trades(dydx_symbol.raw_symbol)

    async def _subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int | None = None,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        if book_type in (BookType.L1_MBP, BookType.L3_MBO):
            self._log.error(
                "Cannot subscribe to order book deltas: L3_MBO data is not published by dYdX. The only valid book type is L2_MBP",
            )
            return

        dydx_symbol = DYDXSymbol(instrument_id.symbol.value)

        # Check if the websocket client is already subscribed.
        subscription = ("v4_orderbook", dydx_symbol.raw_symbol)
        self._orderbook_subscriptions.add(dydx_symbol.raw_symbol)

        if not self._ws_client.has_subscription(subscription):
            await self._ws_client.subscribe_order_book(dydx_symbol.raw_symbol)

    async def _subscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        self._log.debug(
            f"Subscribing deltas {instrument_id} (quotes are not available)",
            LogColor.MAGENTA,
        )
        book_type = BookType.L2_MBP
        self._books[instrument_id] = OrderBook(instrument_id, book_type)

        # Check if the websocket client is already subscribed.
        dydx_symbol = DYDXSymbol(instrument_id.symbol.value)
        subscription = ("v4_orderbook", dydx_symbol.raw_symbol)

        if not self._ws_client.has_subscription(subscription):
            await self._subscribe_order_book_deltas(
                instrument_id=instrument_id,
                book_type=book_type,
            )

    async def _subscribe_bars(self, bar_type: BarType) -> None:
        self._log.info(f"Subscribe to {bar_type} bars")
        dydx_symbol = DYDXSymbol(bar_type.instrument_id.symbol.value)
        candles_resolution = get_interval_from_bar_type(bar_type)
        topic = f"{dydx_symbol.raw_symbol}/{candles_resolution.value}"
        self._topic_bar_type[topic] = bar_type
        await self._ws_client.subscribe_klines(dydx_symbol.raw_symbol, candles_resolution)

    async def _unsubscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        dydx_symbol = DYDXSymbol(instrument_id.symbol.value)
        await self._ws_client.unsubscribe_trades(dydx_symbol.raw_symbol)

    async def _unsubscribe_order_book_deltas(self, instrument_id: InstrumentId) -> None:
        dydx_symbol = DYDXSymbol(instrument_id.symbol.value)

        # Check if the websocket client is subscribed.
        subscription = ("v4_orderbook", dydx_symbol.raw_symbol)

        if dydx_symbol.raw_symbol in self._orderbook_subscriptions:
            self._orderbook_subscriptions.remove(dydx_symbol.raw_symbol)

        if self._ws_client.has_subscription(subscription):
            await self._ws_client.unsubscribe_order_book(dydx_symbol.raw_symbol)

    async def _unsubscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        dydx_symbol = DYDXSymbol(instrument_id.symbol.value)

        # Check if the websocket client is subscribed.
        subscription = ("v4_orderbook", dydx_symbol.raw_symbol)

        if self._ws_client.has_subscription(subscription):
            await self._unsubscribe_order_book_deltas(instrument_id=instrument_id)

    async def _unsubscribe_bars(self, bar_type: BarType) -> None:
        dydx_symbol = DYDXSymbol(bar_type.instrument_id.symbol.value)
        candles_resolution = get_interval_from_bar_type(bar_type)
        await self._ws_client.unsubscribe_klines(dydx_symbol.raw_symbol, candles_resolution)

    def _get_cached_instrument_id(self, symbol: str) -> InstrumentId:
        dydx_symbol = DYDXSymbol(symbol)
        nautilus_instrument_id: InstrumentId = dydx_symbol.to_instrument_id()
        return nautilus_instrument_id

    async def _request_bars(
        self,
        bar_type: BarType,
        limit: int,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        max_bars = 100

        if limit == 0 or limit > max_bars:
            limit = max_bars

        if bar_type.is_internally_aggregated():
            self._log.error(
                f"Cannot request {bar_type}: only historical bars with EXTERNAL aggregation available from dYdX",
            )
            return

        if not bar_type.spec.is_time_aggregated():
            self._log.error(
                f"Cannot request {bar_type}: only time bars are aggregated by dYdX",
            )
            return

        if bar_type.spec.price_type != PriceType.LAST:
            self._log.error(
                f"Cannot request {bar_type}: only historical bars for LAST price type available from dYdX",
            )
            return

        symbol = DYDXSymbol(bar_type.instrument_id.symbol.value)
        instrument_id: InstrumentId = symbol.to_instrument_id()

        instrument = self._cache.instrument(instrument_id)

        if instrument is None:
            self._log.error(f"Cannot parse kline data: no instrument for {instrument_id}")
            return

        candles = await self._http_market.get_candles(
            symbol=symbol,
            resolution=self._enum_parser.parse_dydx_kline(bar_type),
            limit=limit,
            start=start,
            end=end,
        )

        if candles is not None:
            ts_init = self._clock.timestamp_ns()

            bars = [
                candle.parse_to_bar(
                    bar_type=bar_type,
                    price_precision=instrument.price_precision,
                    size_precision=instrument.size_precision,
                    ts_init=ts_init,
                )
                for candle in candles.candles
            ]

            partial: Bar = bars.pop()
            self._handle_bars(bar_type, bars, partial, correlation_id)
