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
Provide a data client for the dYdX decentralized cypto exchange.
"""

import asyncio
from datetime import datetime
from decimal import Decimal
from typing import TYPE_CHECKING

import msgspec
import pandas as pd

from nautilus_trader.adapters.dydx.common.constants import DYDX_VENUE
from nautilus_trader.adapters.dydx.common.enums import DYDXChannel
from nautilus_trader.adapters.dydx.common.enums import DYDXEnumParser
from nautilus_trader.adapters.dydx.common.parsing import get_interval_from_bar_type
from nautilus_trader.adapters.dydx.common.symbol import DYDXSymbol
from nautilus_trader.adapters.dydx.common.types import DYDXOraclePrice
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
from nautilus_trader.data.messages import RequestBars
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
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model import DataType
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Quantity


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
            max_send_retries=config.max_retries or 3,
            delay_initial_ms=config.retry_delay_initial_ms or 100,
            delay_max_ms=config.retry_delay_max_ms or 5_000,
            backoff_factor=2,
        )

        # HTTP API
        self._http_market = DYDXMarketHttpAPI(client=client, clock=clock)

        self._books: dict[InstrumentId, OrderBook] = {}
        self._topic_bar_type: dict[str, BarType] = {}

        self._update_instruments_interval_mins: int | None = config.update_instruments_interval_mins
        self._update_orderbook_interval_secs: int = 60  # Once every 60 seconds (hardcoded for now)
        self._update_instruments_task: asyncio.Task | None = None
        self._fetch_orderbook_task: asyncio.Task | None = None
        self._last_quotes: dict[InstrumentId, QuoteTick] = {}
        self._orderbook_subscriptions: set[str] = set()

        # Hot caches
        self._bars: dict[BarType, Bar] = {}

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        self._send_all_instruments_to_data_engine()

        if self._update_instruments_interval_mins:
            self._update_instruments_task = self.create_task(
                self._update_instruments(self._update_instruments_interval_mins),
            )
        self._fetch_orderbook_task = self.create_task(
            self._fetch_orderbooks_on_interval(),
        )

        self._log.info("Initializing websocket connection")
        await self._ws_client.connect()

        await self._ws_client.subscribe_markets()

    async def _disconnect(self) -> None:
        if self._update_instruments_task:
            self._log.debug("Canceling 'update_instruments' task")
            self._update_instruments_task.cancel()
            self._update_instruments_task = None

        if self._fetch_orderbook_task:
            self._log.debug("Canceling 'fetch_orderbook' task")
            self._fetch_orderbook_task.cancel()
            self._fetch_orderbook_task = None

        await self._ws_client.unsubscribe_markets()
        await self._ws_client.disconnect()

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

    async def _fetch_orderbooks_on_interval(self) -> None:
        """
        Fetch the orderbook on a fixed interval `update_orderbook_interval` to ensure it
        does not become outdated.
        """
        try:
            while True:
                self._log.debug(
                    f"Scheduled `resubscribe_order_book` to run in {self._update_orderbook_interval_secs}s",
                )
                await asyncio.sleep(self._update_orderbook_interval_secs)
                await self._fetch_orderbooks()
        except asyncio.CancelledError:
            self._log.debug("Canceled task 'fetch_orderbook'")

    async def _fetch_orderbooks(self) -> None:
        """
        Request a new orderbook snapshot for all order book subscriptions.
        """
        try:
            tasks = []

            for symbol in self._orderbook_subscriptions:
                tasks.append(self._fetch_orderbook(symbol))

            await asyncio.gather(*tasks)
        except Exception as e:
            self._log.exception("Failed to fetch the orderbooks", e)

    async def _fetch_orderbook(self, symbol: str) -> None:
        """
        Request a new orderbook snapshot.
        """
        try:
            msg = await self._http_market.get_orderbook(symbol=symbol)

            if msg is not None:
                instrument_id: InstrumentId = self._get_cached_instrument_id(symbol)
                instrument = self._cache.instrument(instrument_id)

                if instrument is None:
                    self._log.error(
                        f"Cannot parse orderbook snapshot: no instrument for {instrument_id}",
                    )
                    return

                ts_init = self._clock.timestamp_ns()
                deltas = msg.parse_to_snapshot(
                    instrument_id=instrument_id,
                    price_precision=instrument.price_precision,
                    size_precision=instrument.size_precision,
                    ts_event=ts_init,
                    ts_init=ts_init,
                )

                self._handle_deltas(instrument_id=instrument_id, deltas=deltas)
        except Exception as e:
            self._log.exception(f"Failed to fetch the orderbook for {symbol}", e)

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
            else:
                self._log.error(
                    f"Unknown message `{ws_message.channel}` `{ws_message.type}`: {raw.decode()}",
                )
        except Exception as e:
            self._log.exception(f"Failed to parse websocket message: {raw.decode()}", e)

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
            self._log.exception(f"Failed to parse trade tick: {raw.decode()}", e)

    def _handle_trade_subscribed(self, raw: bytes) -> None:
        # Do not send the historical trades to the DataEngine for the initial subscribed message.
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

            ts_init = self._clock.timestamp_ns()
            deltas = msg.parse_to_deltas(
                instrument_id=instrument_id,
                price_precision=instrument.price_precision,
                size_precision=instrument.size_precision,
                ts_event=ts_init,
                ts_init=ts_init,
            )

            self._handle_deltas(instrument_id=instrument_id, deltas=deltas)

        except Exception as e:
            self._log.exception(f"Failed to parse orderbook: {raw.decode()}", e)

    def _handle_orderbook_batched(self, raw: bytes) -> None:
        try:
            msg = self._decoder_ws_orderbook_batched.decode(raw)

            symbol = msg.id
            instrument_id: InstrumentId = self._get_cached_instrument_id(symbol)

            instrument = self._cache.instrument(instrument_id)

            if instrument is None:
                self._log.error(f"Cannot parse orderbook data: no instrument for {instrument_id}")
                return

            ts_init = self._clock.timestamp_ns()
            deltas = msg.parse_to_deltas(
                instrument_id=instrument_id,
                price_precision=instrument.price_precision,
                size_precision=instrument.size_precision,
                ts_event=ts_init,
                ts_init=ts_init,
            )

            self._handle_deltas(instrument_id=instrument_id, deltas=deltas)

        except Exception as e:
            self._log.exception(f"Failed to parse orderbook: {raw.decode()}", e)

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

            ts_init = self._clock.timestamp_ns()
            deltas = msg.parse_to_snapshot(
                instrument_id=instrument_id,
                price_precision=instrument.price_precision,
                size_precision=instrument.size_precision,
                ts_event=ts_init,
                ts_init=ts_init,
            )

            self._handle_deltas(instrument_id=instrument_id, deltas=deltas)

        except Exception as e:
            self._log.exception(f"Failed to parse orderbook snapshot: {raw.decode()}", e)

    def _resolve_crossed_order_book(
        self,
        book: OrderBook,
        venue_deltas: OrderBookDeltas,
        instrument_id: InstrumentId,
    ) -> OrderBookDeltas:
        """
        Reconcile the order book by generating new deltas when the order book is
        crossed.

        One possible explanation for this behaviour could be that certain orders are not
        acknowledged by other validators in the network. Another more unfortunate
        explanation is that messages are missed or not send by the venue.

        """
        instrument = self._cache.instrument(instrument_id)

        if instrument is None:
            self._log.error(f"Cannot resolve crossed order book: no instrument for {instrument_id}")
            return venue_deltas

        book.apply_deltas(venue_deltas)
        bid_price = book.best_bid_price()
        ask_price = book.best_ask_price()
        bid_size = book.best_bid_size()
        ask_size = book.best_ask_size()

        if bid_price is None or ask_price is None:
            return venue_deltas

        is_order_book_crossed = bid_price >= ask_price
        ts_init = venue_deltas.ts_init
        deltas: list[OrderBookDelta] = venue_deltas.deltas

        while is_order_book_crossed is True:
            self._log.debug("Resolve crossed order book")
            temp_deltas: list[OrderBookDelta] = []

            if bid_size > ask_size:
                # Remove ask price level from book and decrease bid size
                delta = OrderBookDelta(
                    instrument_id=instrument_id,
                    action=BookAction.UPDATE,
                    order=BookOrder(
                        side=OrderSide.BUY,
                        price=bid_price,
                        size=Quantity(bid_size - ask_size, instrument.size_precision),
                        order_id=0,
                    ),
                    flags=0,
                    sequence=0,
                    ts_event=ts_init,
                    ts_init=ts_init,
                )
                temp_deltas.append(delta)

                delta = OrderBookDelta(
                    instrument_id=instrument_id,
                    action=BookAction.DELETE,
                    order=BookOrder(
                        side=OrderSide.SELL,
                        price=ask_price,
                        size=Quantity(0, instrument.size_precision),
                        order_id=0,
                    ),
                    flags=0,
                    sequence=0,
                    ts_event=ts_init,
                    ts_init=ts_init,
                )
                temp_deltas.append(delta)
            elif bid_size < ask_size:
                # Remove bid price level from book and decrease ask size
                delta = OrderBookDelta(
                    instrument_id=instrument_id,
                    action=BookAction.UPDATE,
                    order=BookOrder(
                        side=OrderSide.SELL,
                        price=ask_price,
                        size=Quantity(ask_size - bid_size, instrument.size_precision),
                        order_id=0,
                    ),
                    flags=0,
                    sequence=0,
                    ts_event=ts_init,
                    ts_init=ts_init,
                )
                temp_deltas.append(delta)

                delta = OrderBookDelta(
                    instrument_id=instrument_id,
                    action=BookAction.DELETE,
                    order=BookOrder(
                        side=OrderSide.BUY,
                        price=bid_price,
                        size=Quantity(0, instrument.size_precision),
                        order_id=0,
                    ),
                    flags=0,
                    sequence=0,
                    ts_event=ts_init,
                    ts_init=ts_init,
                )
                temp_deltas.append(delta)
            else:
                # Remove bid price level and ask price level
                delta = OrderBookDelta(
                    instrument_id=instrument_id,
                    action=BookAction.DELETE,
                    order=BookOrder(
                        side=OrderSide.BUY,
                        price=bid_price,
                        size=Quantity(0, instrument.size_precision),
                        order_id=0,
                    ),
                    flags=0,
                    sequence=0,
                    ts_event=ts_init,
                    ts_init=ts_init,
                )
                temp_deltas.append(delta)

                delta = OrderBookDelta(
                    instrument_id=instrument_id,
                    action=BookAction.DELETE,
                    order=BookOrder(
                        side=OrderSide.SELL,
                        price=ask_price,
                        size=Quantity(0, instrument.size_precision),
                        order_id=0,
                    ),
                    flags=0,
                    sequence=0,
                    ts_event=ts_init,
                    ts_init=ts_init,
                )
                temp_deltas.append(delta)

            deltas += temp_deltas
            order_book_deltas = OrderBookDeltas(instrument_id=instrument_id, deltas=temp_deltas)
            book.apply_deltas(order_book_deltas)

            bid_price = book.best_bid_price()
            ask_price = book.best_ask_price()
            bid_size = book.best_bid_size()
            ask_size = book.best_ask_size()

            if bid_price is None or ask_price is None:
                break

            is_order_book_crossed = bid_price >= ask_price

        final_deltas = []

        for delta in deltas[0 : len(deltas) - 1]:
            new_delta = OrderBookDelta(
                instrument_id=delta.instrument_id,
                action=delta.action,
                order=delta.order,
                flags=0,
                sequence=delta.sequence,
                ts_event=delta.ts_event,
                ts_init=delta.ts_init,
            )
            final_deltas.append(new_delta)

        delta = deltas[-1]
        new_delta = OrderBookDelta(
            instrument_id=delta.instrument_id,
            action=delta.action,
            order=delta.order,
            flags=RecordFlag.F_LAST,
            sequence=delta.sequence,
            ts_event=delta.ts_event,
            ts_init=delta.ts_init,
        )
        final_deltas.append(new_delta)

        return OrderBookDeltas(instrument_id=instrument_id, deltas=final_deltas)

    def _handle_deltas(self, instrument_id: InstrumentId, deltas: OrderBookDeltas) -> None:
        book = self._books.get(instrument_id)

        if book is None:
            self._log.error(
                f"Cannot resolve crossed order book: order book not found for {instrument_id}",
            )
            return

        deltas = self._resolve_crossed_order_book(
            book=book,
            instrument_id=instrument_id,
            venue_deltas=deltas,
        )
        self._handle_data(deltas)

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
            self._log.exception(f"Failed to parse kline data: {raw.decode()}", e)

    def _handle_kline_subscribed(self, raw: bytes) -> None:
        # Do not send the historical bars to the DataEngine for the initial subscribed message.
        pass

    def _handle_kline_unsubscribed(self, msg: DYDXWsMessageGeneral) -> None:
        if msg.id is not None:
            self._topic_bar_type.pop(msg.id, None)

    def _handle_markets(self, raw: bytes) -> None:
        try:
            msg: DYDXWsMarketChannelData = self._decoder_ws_instruments.decode(raw)

            if msg.contents.oraclePrices is not None:
                ts_init = self._clock.timestamp_ns()

                for symbol, oracle_price_market in msg.contents.oraclePrices.items():
                    instrument_id = DYDXSymbol(symbol).to_instrument_id()
                    oracle_price = Decimal(oracle_price_market.oraclePrice)

                    instrument = self._cache.instrument(instrument_id)

                    if instrument is None:
                        self._log.debug(
                            f"Cannot parse market message: no instrument for {instrument_id}",
                        )
                        continue

                    dydx_oracle_price = DYDXOraclePrice(
                        instrument_id=instrument_id,
                        price=oracle_price,
                        ts_event=ts_init,
                        ts_init=ts_init,
                    )
                    data_type = DataType(DYDXOraclePrice)
                    self._msgbus.publish(topic=f"data.{data_type.topic}", msg=dydx_oracle_price)

        except Exception as e:
            self._log.exception(f"Failed to parse market data: {raw.decode()}", e)

    def _handle_markets_subscribed(self, raw: bytes) -> None:
        try:
            msg: DYDXWsMarketSubscribedData = self._decoder_ws_instruments_subscribed.decode(raw)
            ts_init = self._clock.timestamp_ns()

            for symbol, oracle_price_market in msg.contents.markets.items():
                if oracle_price_market.oraclePrice is not None:
                    instrument_id = DYDXSymbol(symbol).to_instrument_id()
                    oracle_price = Decimal(oracle_price_market.oraclePrice)

                    instrument = self._cache.instrument(instrument_id)

                    if instrument is None:
                        self._log.debug(
                            f"Cannot parse market message: no instrument for {instrument_id}",
                        )
                        continue

                    dydx_oracle_price = DYDXOraclePrice(
                        instrument_id=instrument_id,
                        price=oracle_price,
                        ts_event=ts_init,
                        ts_init=ts_init,
                    )
                    data_type = DataType(DYDXOraclePrice)
                    self._msgbus.publish(topic=f"data.{data_type.topic}", msg=dydx_oracle_price)

        except Exception as e:
            self._log.exception(f"Failed to parse market channel data: {raw.decode()}", e)

    async def _subscribe_instruments(self, command: SubscribeInstruments) -> None:
        """
        Subscribe to instruments updates.

        Parameters
        ----------
        command : SubscribeInstruments
            The command to subscribe to instruments.

        """
        self._log.info("Skipping subscribe_instruments, dYdX subscribes automatically")

    async def _subscribe_instrument(self, command: SubscribeInstrument) -> None:
        """
        Subscribe to instrument updates.

        Parameters
        ----------
        command : SubscribeInstrument
            The command to subscribe to instrument.

        """
        self._log.info("Skipping subscribe_instrument, dYdX subscribes automatically")

    async def _unsubscribe_instruments(
        self,
        command: UnsubscribeInstruments,
    ) -> None:
        """
        Unsubscribe from instruments updates.

        Parameters
        ----------
        command : UnsubscribeInstruments
            The command to unsubscribe from instruments updates.

        """
        self._log.info("Skipping unsubscribe_instruments, not applicable for dYdX")

    async def _unsubscribe_instrument(self, command: UnsubscribeInstrument) -> None:
        """
        Unsubscribe from instrument updates.

        Parameters
        ----------
        command : UnsubscribeInstrument
            The command to unsubscribe from instrument updates.

        """
        self._log.info("Skipping unsubscribe_instrument, not applicable for dYdX")

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        dydx_symbol = DYDXSymbol(command.instrument_id.symbol.value)
        await self._ws_client.subscribe_trades(dydx_symbol.raw_symbol)

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        if command.book_type in (BookType.L1_MBP, BookType.L3_MBO):
            self._log.error(
                "Cannot subscribe to order book deltas: L3_MBO data is not published by dYdX. The only valid book type is L2_MBP",
            )
            return

        dydx_symbol = DYDXSymbol(command.instrument_id.symbol.value)

        # Check if the websocket client is already subscribed.
        self._orderbook_subscriptions.add(dydx_symbol.raw_symbol)

        if command.instrument_id not in self._books:
            self._books[command.instrument_id] = OrderBook(command.instrument_id, command.book_type)

        if not self._ws_client.has_subscription(
            channel=DYDXChannel.ORDERBOOK,
            channel_id=dydx_symbol.raw_symbol,
        ):
            await self._ws_client.subscribe_order_book(dydx_symbol.raw_symbol)

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        self._log.debug(
            f"Subscribing deltas {command.instrument_id} (quotes are not available)",
            LogColor.MAGENTA,
        )
        book_type = BookType.L2_MBP

        # Check if the websocket client is already subscribed.
        dydx_symbol = DYDXSymbol(command.instrument_id.symbol.value)

        if not self._ws_client.has_subscription(
            channel=DYDXChannel.ORDERBOOK,
            channel_id=dydx_symbol.raw_symbol,
        ):
            order_book_command = SubscribeOrderBook(
                command_id=command.id,
                instrument_id=command.instrument_id,
                book_type=book_type,
                book_data_type=OrderBookDelta,
                client_id=command.client_id,
                venue=command.venue,
                ts_init=command.ts_init,
                params=command.params,
            )
            await self._subscribe_order_book_deltas(order_book_command)

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        self._log.info(f"Subscribe to {command.bar_type} bars")
        dydx_symbol = DYDXSymbol(command.bar_type.instrument_id.symbol.value)
        candles_resolution = get_interval_from_bar_type(command.bar_type)
        topic = f"{dydx_symbol.raw_symbol}/{candles_resolution.value}"
        self._topic_bar_type[topic] = command.bar_type
        await self._ws_client.subscribe_klines(dydx_symbol.raw_symbol, candles_resolution)

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        dydx_symbol = DYDXSymbol(command.instrument_id.symbol.value)
        await self._ws_client.unsubscribe_trades(dydx_symbol.raw_symbol)

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        dydx_symbol = DYDXSymbol(command.instrument_id.symbol.value)

        # Check if the websocket client is subscribed.
        if dydx_symbol.raw_symbol in self._orderbook_subscriptions:
            self._orderbook_subscriptions.remove(dydx_symbol.raw_symbol)

        if self._ws_client.has_subscription(
            channel=DYDXChannel.ORDERBOOK,
            channel_id=dydx_symbol.raw_symbol,
        ):
            await self._ws_client.unsubscribe_order_book(dydx_symbol.raw_symbol)

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        dydx_symbol = DYDXSymbol(command.instrument_id.symbol.value)

        # Check if the websocket client is subscribed.
        if self._ws_client.has_subscription(
            channel=DYDXChannel.ORDERBOOK,
            channel_id=dydx_symbol.raw_symbol,
        ):
            order_book_command = UnsubscribeOrderBook(
                command_id=command.id,
                instrument_id=command.instrument_id,
                book_data_type=OrderBookDelta,
                client_id=command.client_id,
                venue=command.venue,
                ts_init=command.ts_init,
                params=command.params,
            )
            await self._unsubscribe_order_book_deltas(order_book_command)

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        dydx_symbol = DYDXSymbol(command.bar_type.instrument_id.symbol.value)
        candles_resolution = get_interval_from_bar_type(command.bar_type)
        await self._ws_client.unsubscribe_klines(dydx_symbol.raw_symbol, candles_resolution)

    def _get_cached_instrument_id(self, symbol: str) -> InstrumentId:
        dydx_symbol = DYDXSymbol(symbol)
        nautilus_instrument_id: InstrumentId = dydx_symbol.to_instrument_id()
        return nautilus_instrument_id

    def _should_partition_bars_request(self, request: RequestBars, max_bars: int) -> bool:
        bar_timedelta = request.bar_type.spec.timedelta
        total_duration = request.end - request.start
        expected_bars = int(total_duration / bar_timedelta)
        return expected_bars > max_bars

    async def _fetch_candles(
        self,
        symbol: DYDXSymbol,
        bar_type: BarType,
        instrument: Instrument,
        start: datetime | pd.Timestamp | None,
        end: datetime | pd.Timestamp | None,
        request_limit: int,
    ) -> list[Bar]:
        """
        Fetch candles from API and convert to bars.
        """
        candles = await self._http_market.get_candles(
            symbol=symbol,
            resolution=self._enum_parser.parse_dydx_kline(bar_type),
            limit=request_limit,
            start=start,
            end=end,
        )

        if candles is None or not candles.candles:
            return []

        ts_init = self._clock.timestamp_ns()
        return [
            candle.parse_to_bar(
                bar_type=bar_type,
                price_precision=instrument.price_precision,
                size_precision=instrument.size_precision,
                ts_init=ts_init,
            )
            for candle in candles.candles
        ]

    async def _request_bars(self, request: RequestBars) -> None:
        max_bars = 1000

        if request.bar_type.is_internally_aggregated():
            self._log.error(
                f"Cannot request {request.bar_type} bars: only historical bars with EXTERNAL aggregation available from dYdX",
            )
            return

        if not request.bar_type.spec.is_time_aggregated():
            self._log.error(
                f"Cannot request {request.bar_type} bars: only time bars are aggregated by dYdX",
            )
            return

        if request.bar_type.spec.price_type != PriceType.LAST:
            self._log.error(
                f"Cannot request {request.bar_type} bars: only historical bars for LAST price type available from dYdX",
            )
            return

        symbol = DYDXSymbol(request.bar_type.instrument_id.symbol.value)
        instrument_id: InstrumentId = symbol.to_instrument_id()

        instrument = self._cache.instrument(instrument_id)

        if instrument is None:
            self._log.error(f"Cannot parse kline data: no instrument for {instrument_id}")
            return

        all_bars = []

        # Check if we need to partition the request
        if (
            request.start is not None
            and request.end is not None
            and self._should_partition_bars_request(request, max_bars)
        ):
            # Partition into multiple requests
            self._log.info(
                f"Expected bars exceed limit of {max_bars}, partitioning into multiple requests",
            )

            chunk_duration = request.bar_type.spec.timedelta * max_bars
            current_start = request.start

            while current_start < request.end:
                current_end = min(current_start + chunk_duration, request.end)

                chunk_bars = await self._fetch_candles(
                    symbol,
                    request.bar_type,
                    instrument,
                    current_start,
                    current_end,
                    max_bars,
                )
                all_bars.extend(chunk_bars)

                current_start = current_end

                # Apply overall limit if specified
                if request.limit > 0 and len(all_bars) >= request.limit:
                    all_bars = all_bars[: request.limit]
                    break
        else:
            # Single request
            limit = request.limit if request.limit > 0 else max_bars
            all_bars = await self._fetch_candles(
                symbol,
                request.bar_type,
                instrument,
                request.start,
                request.end,
                limit,
            )

        if all_bars:
            # Filter out incomplete bars where close_time >= current_time
            # dYdX may return the current forming bar which should be excluded from historical data
            current_time_ns = self._clock.timestamp_ns()
            complete_bars = [bar for bar in all_bars if bar.ts_event < current_time_ns]

            if not complete_bars:
                self._log.warning(
                    f"No complete bars available for {request.bar_type} (all bars were incomplete)",
                )
                return

            self._handle_bars_py(
                request.bar_type,
                complete_bars,
                request.id,
                request.start,
                request.end,
                request.params,
            )
