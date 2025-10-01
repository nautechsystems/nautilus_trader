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
import copy
import datetime as dt
from typing import Any

from msgspec import json as msgspec_json
from schwab.streaming import StreamClient

from nautilus_trader.adapters.schwab.common import SCHWAB_VENUE
from nautilus_trader.adapters.schwab.config import SchwabDataClientConfig
from nautilus_trader.adapters.schwab.http.client import SchwabHttpClient
from nautilus_trader.adapters.schwab.providers import SchwabInstrumentProvider
from nautilus_trader.adapters.schwab.websocket.client import SchwabWebSocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.datetime import millis_to_nanos
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
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class SchwabDataClient(LiveMarketDataClient):
    """
    Schwab market data client based on ``schwab-py``.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        http_client: SchwabHttpClient,
        instrument_provider: SchwabInstrumentProvider,
        config: SchwabDataClientConfig,
        name: str | None = None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or f"{SCHWAB_VENUE.value}-DATA"),
            venue=SCHWAB_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        self._config = config
        self._http_client = http_client
        self._bars_timestamp_on_close = config.bars_timestamp_on_close
        # WebSocket API
        self._ws_client = SchwabWebSocketClient(
            clock=clock,
            http_client=self._http_client,
            handler=self._handle_ws_message,
            handler_reconnect=None,
            loop=self._loop,
        )
        self._ws_handlers_map = {
            "CHART_EQUITY": self._handle_chart_equity_message,
            "CHART_FUTURES": self._handle_chart_futures_message,
            "LEVELONE_EQUITIES": self._handle_level_one_equity_message,
            "NYSE_BOOK": self._handle_level_two_message,
            "NASDAQ_BOOK": self._handle_level_two_message,
            "SMART_BOOK": self._handle_level_two_message,
        }
        self._subscribe_bar_types: dict[str, BarType] = {}
        self._level_one_instrument_id_cache: dict[str, InstrumentId] = {}
        self._order_book_deltas_instrument_id_cache: dict[str, InstrumentId] = {}
        self._last_quotes: dict[InstrumentId, QuoteTick] = {}
        self._last_trades: dict[InstrumentId, TradeTick] = {}

    def _label_message(self, msg, field_enum_type):
        if "content" in msg:
            new_msg = copy.deepcopy(msg)
            for idx in range(len(msg["content"])):
                field_enum_type.relabel_message(
                    msg["content"][idx],
                    new_msg["content"][idx],
                )
            return new_msg
        else:
            return msg

    def _handle_chart_equity_message(self, msg: dict[str, Any]) -> None:
        labeled_msg = self._label_message(msg, StreamClient.ChartEquityFields)
        try:
            for data in labeled_msg["content"]:
                # Only 1 minute bar is supported by schwab for now
                bar_type = self._subscribe_bar_types.get(data["key"], None)
                if bar_type is None:
                    self._log.error(
                        f"Cannot parse bar data: no bar_type for {msg}",
                    )
                    return
                instrument_id = bar_type.instrument_id
                instrument = self._cache.instrument(instrument_id)
                ts_event = millis_to_nanos(data["CHART_TIME_MILLIS"])
                if self._bars_timestamp_on_close:
                    interval_ms = bar_type.spec.timedelta / dt.timedelta(milliseconds=1)
                    ts_event += millis_to_nanos(interval_ms)
                bar = Bar(
                    bar_type=bar_type,
                    open=Price(data["OPEN_PRICE"], instrument.price_precision),
                    high=Price(data["HIGH_PRICE"], instrument.price_precision),
                    low=Price(data["LOW_PRICE"], instrument.price_precision),
                    close=Price(
                        data["CLOSE_PRICE"],
                        instrument.price_precision,
                    ),
                    volume=Quantity(data["VOLUME"], instrument.size_precision),
                    ts_event=ts_event,
                    ts_init=self._clock.timestamp_ns(),
                )
                self._handle_data(bar)
        except Exception as e:
            self._log.exception(f"Failed to parse bar: {msg}", e)

    def _handle_chart_futures_message(self, msg: dict[str, Any]) -> None:
        pass

    def _handle_level_one_equity_message(self, msg: dict[str, Any]) -> None:
        labeled_msg = self._label_message(
            msg,
            StreamClient.LevelOneEquityFields,
        )
        try:
            for data in labeled_msg["content"]:
                instrument_id = self._level_one_instrument_id_cache[data["key"]]
                instrument = self._cache.instrument(instrument_id)
                last_quote = self._last_quotes.get(instrument_id, None)
                if last_quote:
                    bid_price = data.get(
                        "BID_PRICE",
                        last_quote.bid_price.as_double(),
                    )
                    ask_price = data.get(
                        "ASK_PRICE",
                        last_quote.ask_price.as_double(),
                    )
                    bid_size = data.get(
                        "BID_SIZE",
                        last_quote.bid_size.as_double(),
                    )
                    ask_size = data.get(
                        "ASK_SIZE",
                        last_quote.ask_size.as_double(),
                    )
                else:
                    bid_price = data["BID_PRICE"]
                    ask_price = data["ASK_PRICE"]
                    bid_size = data["BID_SIZE"]
                    ask_size = data["ASK_SIZE"]
                ts_event = data.get(
                    "QUOTE_TIME_MILLIS",
                    data.get("TRADE_TIME_MILLIS", None),
                )
                if ts_event is None:
                    ts_event = labeled_msg["timestamp"]
                tick = QuoteTick(
                    instrument_id=instrument_id,
                    bid_price=Price(bid_price, instrument.price_precision),
                    ask_price=Price(ask_price, instrument.price_precision),
                    bid_size=Quantity(bid_size, instrument.size_precision),
                    ask_size=Quantity(ask_size, instrument.size_precision),
                    ts_event=millis_to_nanos(ts_event),
                    ts_init=self._clock.timestamp_ns(),
                )
                self._last_quotes[instrument_id] = tick
                self._handle_data(tick)
        except Exception as e:
            self._log.exception(f"Failed to parse level one tick: {msg}", e)

    def _label_book_message(self, msg: dict[str, Any]) -> None:
        new_msg = self._label_message(msg, StreamClient.BookFields)

        # Relabel bids
        for content in new_msg["content"]:
            if "BIDS" in content:
                for bid in content["BIDS"]:
                    # Relabel top-level bids
                    StreamClient.BidFields.relabel_message(bid, bid)

                    # Relabel per-exchange bids
                    for e_bid in bid["BIDS"]:
                        StreamClient.PerExchangeBidFields.relabel_message(
                            e_bid,
                            e_bid,
                        )

        # Relabel asks
        for content in new_msg["content"]:
            if "ASKS" in content:
                for ask in content["ASKS"]:
                    # Relabel top-level asks
                    StreamClient.AskFields.relabel_message(ask, ask)

                    # Relabel per-exchange bids
                    for e_ask in ask["ASKS"]:
                        StreamClient.PerExchangeAskFields.relabel_message(
                            e_ask,
                            e_ask,
                        )

        return new_msg

    def _handle_level_two_message(self, msg: dict[str, Any]) -> None:
        labeled_msg = self._label_book_message(msg)
        try:
            for data in labeled_msg["content"]:
                instrument_id = self._order_book_deltas_instrument_id_cache[data["key"]]
                instrument = self._cache.instrument(instrument_id)
                ts_init = self._clock.timestamp_ns()
                ts_event = millis_to_nanos(data["BOOK_TIME"])
                deltas: list[OrderBookDelta] = []
                deltas.append(
                    OrderBookDelta.clear(
                        instrument_id,
                        0,
                        ts_event,
                        ts_init,
                    ),
                )
                for bid in data["BIDS"]:
                    deltas.append(
                        OrderBookDelta(
                            instrument_id=instrument_id,
                            action=BookAction.ADD,
                            order=BookOrder(
                                side=OrderSide.BUY,
                                price=Price(
                                    bid["BID_PRICE"],
                                    instrument.price_precision,
                                ),
                                size=Quantity.from_int(bid["NUM_BIDS"]),
                                order_id=0,
                            ),
                            flags=0,
                            sequence=0,
                            ts_event=ts_event,
                            ts_init=ts_init,
                        ),
                    )
                for ask in data["ASKS"]:
                    deltas.append(
                        OrderBookDelta(
                            instrument_id=instrument_id,
                            action=BookAction.ADD,
                            order=BookOrder(
                                side=OrderSide.SELL,
                                price=Price(
                                    ask["ASK_PRICE"],
                                    instrument.price_precision,
                                ),
                                size=Quantity.from_int(ask["NUM_ASKS"]),
                                order_id=0,
                            ),
                            flags=0,
                            sequence=0,
                            ts_event=ts_event,
                            ts_init=ts_init,
                        ),
                    )
                snapshot = OrderBookDeltas(
                    instrument_id=instrument_id,
                    deltas=deltas,
                )
                self._handle_data(snapshot)
        except Exception as e:
            self._log.exception(f"Failed to parse level two tick: {msg}", e)

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        await self._ws_client.connect()
        self._send_all_instruments_to_data_engine()

    async def _disconnect(self) -> None:
        self._last_quotes.clear()
        self._last_trades.clear()

    def _handle_ws_message(self, raw: bytes) -> None:
        msg = msgspec_json.decode(raw)
        self._log.debug(f"Received: {msg}", LogColor.BLUE)
        if "notify" in msg:
            for d in msg["notify"]:
                if "heartbeat" in d:
                    continue
                else:
                    if d["service"] in self._ws_handlers_map:
                        self._ws_handlers_map[d["service"]](d)

        if "data" in msg:
            for d in msg["data"]:
                if d["service"] in self._ws_handlers_map:
                    self._ws_handlers_map[d["service"]](d)

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        symbol = command.instrument_id.symbol.value
        self._level_one_instrument_id_cache[symbol] = command.instrument_id
        await self._ws_client.subscribe_quote_ticks(symbol)

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        symbol = command.instrument_id.symbol.value
        self._level_one_instrument_id_cache.pop(symbol, None)
        await self._ws_client.unsubscribe_quote_ticks(symbol)

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        raise NotImplementedError

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        raise NotImplementedError

    async def _request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        raise NotImplementedError

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        raise NotImplementedError

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        symbol = command.bar_type.instrument_id.symbol.value
        interval_str = str(command.bar_type.spec)
        self._subscribe_bar_types[symbol] = command.bar_type
        await self._ws_client.subscribe_klines(symbol, interval_str)

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        symbol = command.bar_type.instrument_id.symbol.value
        interval_str = str(command.bar_type.spec)
        self._subscribe_bar_types.pop(symbol, None)
        await self._ws_client.unsubscribe_klines(symbol, interval_str)

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        if command.book_type != BookType.L2_MBP:
            self._log.error(
                "Cannot subscribe to order book deltas: " "Only L2_MBP data is supported by Schwab",
            )
            return
        symbol = command.instrument_id.symbol.value
        venue = command.instrument_id.venue.value
        self._order_book_deltas_instrument_id_cache[symbol] = command.instrument_id
        await self._ws_client.subscribe_order_book_deltas(symbol, venue)

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        symbol = command.instrument_id.symbol.value
        venue = command.instrument_id.venue.value
        self._order_book_deltas_instrument_id_cache.pop(symbol, None)
        await self._ws_client.unsubscribe_order_book_deltas(symbol, venue)

    def _send_all_instruments_to_data_engine(self) -> None:
        instruments = self._instrument_provider.get_all().values()
        for instrument in instruments:
            self._handle_data(instrument)

        for currency in self._instrument_provider.currencies().values():
            self._cache.add_currency(currency)


__all__ = ["SchwabDataClient"]
