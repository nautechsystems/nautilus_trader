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

import asyncio

import msgspec
import pandas as pd

from nautilus_trader.adapters.bybit.common.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.common.enums import BybitEnumParser
from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.adapters.bybit.config import BybitDataClientConfig
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.http.market import BybitMarketHttpAPI
from nautilus_trader.adapters.bybit.provider import BybitInstrumentProvider
from nautilus_trader.adapters.bybit.schemas.market.ticker import BybitTickerData
from nautilus_trader.adapters.bybit.schemas.symbol import BybitSymbol
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsMessageGeneral
from nautilus_trader.adapters.bybit.schemas.ws import decoder_ws_kline
from nautilus_trader.adapters.bybit.schemas.ws import decoder_ws_orderbook
from nautilus_trader.adapters.bybit.schemas.ws import decoder_ws_ticker
from nautilus_trader.adapters.bybit.schemas.ws import decoder_ws_trade
from nautilus_trader.adapters.bybit.utils import get_api_key
from nautilus_trader.adapters.bybit.utils import get_api_secret
from nautilus_trader.adapters.bybit.utils import get_interval_from_bar_type
from nautilus_trader.adapters.bybit.websocket.client import BybitWebsocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.datetime import secs_to_millis
from nautilus_trader.core.message import Request
from nautilus_trader.core.nautilus_pyo3 import Symbol
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.messages import DataResponse
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import CustomData
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class BybitDataClient(LiveMarketDataClient):
    """
    Provides a data client for the `Bybit` exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : BybitHttpClient
        The Bybit HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : BybitInstrumentProvider
        The instrument provider.
    instrument_types : list[BybitInstrumentType]
        The instrument types for the client.
    ws_urls : dict[BybitInstrumentType, str]
        The base urls for the WebSocket clients.
    config : BybitDataClientConfig
        The configuration for the client.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BybitHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: BybitInstrumentProvider,
        instrument_types: list[BybitInstrumentType],
        ws_urls: dict[BybitInstrumentType, str],
        config: BybitDataClientConfig,
    ) -> None:
        self._instrument_types = instrument_types
        self._enum_parser = BybitEnumParser()
        super().__init__(
            loop=loop,
            client_id=ClientId(BYBIT_VENUE.value),
            venue=BYBIT_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        # Hot cache
        self._instrument_ids: dict[str, InstrumentId] = {}
        self._last_quotes: dict[InstrumentId, QuoteTick] = {}

        # HTTP API
        self._http_market = BybitMarketHttpAPI(
            client=client,
            clock=clock,
        )

        # WebSocket API
        self._ws_clients: dict[BybitInstrumentType, BybitWebsocketClient] = {}
        for instrument_type in instrument_types:
            self._ws_clients[instrument_type] = BybitWebsocketClient(
                clock=clock,
                handler=lambda x: self._handle_ws_message(instrument_type, x),
                base_url=ws_urls[instrument_type],
                api_key=config.api_key or get_api_key(config.testnet),
                api_secret=config.api_secret or get_api_secret(config.testnet),
            )

            # WebSocket decoders
            self._decoders = {
                "orderbook": decoder_ws_orderbook(),
                "trade": decoder_ws_trade(),
                "ticker": decoder_ws_ticker(instrument_type),
                "kline": decoder_ws_kline(),
            }
            self._decoder_ws_msg_general = msgspec.json.Decoder(BybitWsMessageGeneral)

        self._topic_bar_type: dict[str, BarType] = {}

        self._update_instrument_interval: int = 60 * 60  # Once per hour (hardcode)
        self._update_instruments_task: asyncio.Task | None = None

        # Register custom endpoint for fetching tickers
        self._msgbus.register(
            endpoint="bybit.data.tickers",
            handler=self.complete_fetch_tickers_task,
        )

    async def fetch_send_tickers(
        self,
        id: UUID4,
        instrument_type: BybitInstrumentType,
        symbol: str,
    ) -> None:
        tickers = await self._http_market.fetch_tickers(
            instrument_type=instrument_type,
            symbol=symbol,
        )
        data = DataResponse(
            client_id=ClientId(BYBIT_VENUE.value),
            venue=BYBIT_VENUE,
            data_type=DataType(CustomData),
            data=tickers,
            correlation_id=id,
            response_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )
        self._msgbus.response(data)

    def complete_fetch_tickers_task(self, request: Request) -> None:
        # Extract symbol from metadata
        if "symbol" not in request.metadata:
            raise ValueError("Symbol not in request metadata")
        symbol = request.metadata["symbol"]
        if not isinstance(symbol, Symbol):
            raise ValueError(
                f"Parameter symbol in request metadata object is not of type Symbol, got {type(symbol)}",
            )
        bybit_symbol = BybitSymbol(symbol.value)
        assert bybit_symbol  # type checking
        self._loop.create_task(
            self.fetch_send_tickers(
                request.id,
                bybit_symbol.instrument_type,
                bybit_symbol.raw_symbol,
            ),
        )

    async def _connect(self) -> None:
        self._log.info("Initializing instruments...")
        await self._instrument_provider.initialize()

        self._send_all_instruments_to_data_engine()
        self._update_instruments_task = self.create_task(self._update_instruments())
        self._log.info("Initializing websocket connections")
        for ws_client in self._ws_clients.values():
            await ws_client.connect()
        self._log.info("Data client connected")

    async def _disconnect(self) -> None:
        if self._update_instruments_task:
            self._log.debug("Cancelling `update_instruments` task")
            self._update_instruments_task.cancel()
            self._update_instruments_task = None
        for ws_client in self._ws_clients.values():
            await ws_client.disconnect()

    def _send_all_instruments_to_data_engine(self) -> None:
        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)

        for currency in self._instrument_provider.currencies().values():
            self._cache.add_currency(currency)

    async def _update_instruments(self) -> None:
        try:
            while True:
                self._log.debug(
                    f"Scheduled `update_instruments` to run in "
                    f"{self._update_instrument_interval}s",
                )
                await asyncio.sleep(self._update_instrument_interval)
                await self._instrument_provider.load_all_async()
                self._send_all_instruments_to_data_engine()
        except asyncio.CancelledError:
            self._log.debug("Canceled `update_instruments` task")

    async def _subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int | None = None,
        kwargs: dict | None = None,
    ) -> None:
        if book_type == BookType.L3_MBO:
            self._log.error(
                "Cannot subscribe to order book deltas: "
                "L3_MBO data is not published by Binance. "
                "Valid book types are L1_MBP, L2_MBP",
            )
            return

        bybit_symbol = BybitSymbol(instrument_id.symbol.value)
        assert bybit_symbol  # type checking
        ws_client = self._ws_clients[bybit_symbol.instrument_type]
        await ws_client.subscribe_order_book(bybit_symbol.raw_symbol, depth or 50)

    async def _subscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        bybit_symbol = BybitSymbol(instrument_id.symbol.value)
        assert bybit_symbol  # type checking
        ws_client = self._ws_clients[bybit_symbol.instrument_type]
        await ws_client.subscribe_tickers(bybit_symbol.raw_symbol)

    async def _subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        bybit_symbol = BybitSymbol(instrument_id.symbol.value)
        assert bybit_symbol  # type checking
        ws_client = self._ws_clients[bybit_symbol.instrument_type]
        await ws_client.subscribe_trades(bybit_symbol.raw_symbol)

    async def _subscribe_bars(self, bar_type: BarType) -> None:
        bybit_symbol = BybitSymbol(bar_type.instrument_id.symbol.value)
        assert bybit_symbol  # type checking
        interval_str = get_interval_from_bar_type(bar_type)
        topic = f"kline.{interval_str}.{bybit_symbol.raw_symbol}"
        self._topic_bar_type[topic] = bar_type
        ws_client = self._ws_clients[bybit_symbol.instrument_type]
        await ws_client.subscribe_klines(bybit_symbol.raw_symbol, interval_str)

    async def _unsubscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        bybit_symbol = BybitSymbol(instrument_id.symbol.value)
        assert bybit_symbol  # type checking
        ws_client = self._ws_clients[bybit_symbol.instrument_type]
        await ws_client.unsubscribe_tickers(bybit_symbol.raw_symbol)

    async def _unsubscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        bybit_symbol = BybitSymbol(instrument_id.symbol.value)
        assert bybit_symbol  # type checking
        ws_client = self._ws_clients[bybit_symbol.instrument_type]
        await ws_client.unsubscribe_trades(bybit_symbol.raw_symbol)

    async def _unsubscribe_bars(self, bar_type: BarType) -> None:
        bybit_symbol = BybitSymbol(bar_type.instrument_id.symbol.value)
        assert bybit_symbol  # type checking
        interval_str = get_interval_from_bar_type(bar_type)
        topic = f"kline.{interval_str}.{bybit_symbol.raw_symbol}"
        self._topic_bar_type.pop(topic, None)
        ws_client = self._ws_clients[bybit_symbol.instrument_type]
        await ws_client.unsubscribe_klines(bybit_symbol.raw_symbol, interval_str)

    def _get_cached_instrument_id(self, symbol: str) -> InstrumentId:
        # Parse instrument ID
        bybit_symbol = BybitSymbol(symbol)
        assert bybit_symbol  # type checking
        nautilus_instrument_id: InstrumentId = bybit_symbol.parse_as_nautilus()
        return nautilus_instrument_id

    async def _request(self, data_type: DataType, correlation_id: UUID4) -> None:
        if data_type.type == BybitTickerData:
            symbol = data_type.metadata["symbol"]
            await self._handle_ticker_data_request(symbol, correlation_id)

    async def _request_instrument(
        self,
        instrument_id: InstrumentId,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        if start is not None:
            self._log.warning(
                f"Requesting instrument {instrument_id} with specified `start` which has no effect",
            )

        if end is not None:
            self._log.warning(
                f"Requesting instrument {instrument_id} with specified `end` which has no effect",
            )

        instrument: Instrument | None = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find instrument for {instrument_id}")
            return
        data_type = DataType(
            type=Instrument,
            metadata={"instrument_id": instrument_id},
        )
        self._handle_data_response(
            data_type=data_type,
            data=instrument,
            correlation_id=correlation_id,
        )

    async def _request_instruments(
        self,
        venue: Venue,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        if start is not None:
            self._log.warning(
                f"Requesting instruments for {venue} with specified `start` which has no effect",
            )

        if end is not None:
            self._log.warning(
                f"Requesting instruments for {venue} with specified `end` which has no effect",
            )

        all_instruments = self._instrument_provider.get_all()
        target_instruments = []
        for instrument in all_instruments.values():
            if instrument.venue == venue:
                target_instruments.append(instrument)
        data_type = DataType(
            type=Instrument,
            metadata={"venue": venue},
        )
        self._handle_data_response(
            data_type=data_type,
            data=target_instruments,
            correlation_id=correlation_id,
        )

    async def _request_quote_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        self._log.error(
            "Cannot request historical quote ticks: not published by Bybit",
        )

    async def _request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        if limit == 0 or limit > 1000:
            limit = 1000

        if start is not None:
            self._log.error(
                "Cannot specify `start` for historical trade ticks: Bybit only provides 'recent trades'",
            )
        if end is not None:
            self._log.error(
                "Cannot specify `end` for historical trade ticks: Bybit only provides 'recent trades'",
            )

        trades = await self._http_market.request_bybit_trades(
            instrument_id=instrument_id,
            limit=limit,
            ts_init=self._clock.timestamp_ns(),
        )
        self._handle_trade_ticks(instrument_id, trades, correlation_id)

    async def _request_bars(
        self,
        bar_type: BarType,
        limit: int,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        if limit == 0 or limit > 1000:
            limit = 1000

        if bar_type.is_internally_aggregated():
            self._log.error(
                f"Cannot request {bar_type}: "
                f"only historical bars with EXTERNAL aggregation available from Bybit",
            )
            return

        if not bar_type.spec.is_time_aggregated():
            self._log.error(
                f"Cannot request {bar_type}: only time bars are aggregated by Bybit",
            )
            return

        if bar_type.spec.price_type != PriceType.LAST:
            self._log.error(
                f"Cannot request {bar_type}: "
                f"only historical bars for LAST price type available from Bybit",
            )
            return

        bybit_interval = self._enum_parser.parse_bybit_kline(bar_type)
        start_time_ms = None
        if start is not None:
            start_time_ms = secs_to_millis(start.timestamp())
        end_time_ms = None
        if end is not None:
            end_time_ms = secs_to_millis(end.timestamp())

        bars = await self._http_market.request_bybit_bars(
            bar_type=bar_type,
            interval=bybit_interval,
            start=start_time_ms,
            end=end_time_ms,
            limit=limit,
            ts_init=self._clock.timestamp_ns(),
        )
        partial: Bar = bars.pop()
        self._handle_bars(bar_type, bars, partial, correlation_id)

    async def _handle_ticker_data_request(self, symbol: Symbol, correlation_id: UUID4) -> None:
        bybit_symbol = BybitSymbol(symbol.value)
        assert bybit_symbol  # type checking
        bybit_tickers = await self._http_market.fetch_tickers(
            instrument_type=bybit_symbol.instrument_type,
            symbol=bybit_symbol.raw_symbol,
        )
        data_type = DataType(
            type=BybitTickerData,
            metadata={"symbol": symbol},
        )
        result = []
        for ticker in bybit_tickers:
            ticker_data: BybitTickerData = BybitTickerData(
                symbol=ticker.symbol,
                bid1Price=ticker.bid1Price,
                bid1Size=ticker.bid1Size,
                ask1Price=ticker.ask1Price,
                ask1Size=ticker.ask1Size,
                lastPrice=ticker.lastPrice,
                highPrice24h=ticker.highPrice24h,
                lowPrice24h=ticker.lowPrice24h,
                turnover24h=ticker.turnover24h,
                volume24h=ticker.volume24h,
            )
            result.append(ticker_data)
        self._handle_data_response(
            data_type,
            result,
            correlation_id,
        )

    def _topic_check(self, instrument_type: BybitInstrumentType, topic: str, raw: bytes) -> None:
        if "orderbook" in topic:
            self._handle_orderbook(instrument_type, raw)
        elif "publicTrade" in topic:
            self._handle_trade(instrument_type, raw)
        elif "tickers" in topic:
            self._handle_ticker(instrument_type, raw)
        elif "kline" in topic:
            self._handle_kline(raw)
        else:
            self._log.error(f"Unknown websocket message topic: {topic} in Bybit")

    def _handle_ws_message(self, instrument_type: BybitInstrumentType, raw: bytes) -> None:
        try:
            ws_message = self._decoder_ws_msg_general.decode(raw)
            if ws_message.success is False:
                self._log.error(f"Error in ws_message: {ws_message.ret_msg}")
                return
            ## Check if there is topic, if not discard the message
            if ws_message.topic:
                self._topic_check(instrument_type, ws_message.topic, raw)
        except Exception as e:
            self._log.error(f"Failed to parse websocket message: {raw.decode()} with error {e}")

    def _handle_orderbook(self, instrument_type: InstrumentId, raw: bytes) -> None:
        msg = self._decoders["orderbook"].decode(raw)
        symbol = msg.data.s + f"-{instrument_type.value.upper()}"
        instrument_id: InstrumentId = self._get_cached_instrument_id(symbol)
        if msg.type == "snapshot":
            deltas: OrderBookDeltas = msg.data.parse_to_snapshot(
                instrument_id=instrument_id,
                ts_event=millis_to_nanos(msg.ts),
                ts_init=self._clock.timestamp_ns(),
            )
        else:
            deltas = msg.data.parse_to_deltas(
                instrument_id=instrument_id,
                ts_event=millis_to_nanos(msg.ts),
                ts_init=self._clock.timestamp_ns(),
            )
        self._handle_data(deltas)

    def _handle_ticker(self, instrument_type: BybitInstrumentType, raw: bytes) -> None:
        msg = self._decoders["ticker"].decode(raw)
        try:
            symbol = msg.data.symbol + f"-{instrument_type.value.upper()}"
            instrument_id: InstrumentId = self._get_cached_instrument_id(symbol)
            last_quote = self._last_quotes.get(instrument_id)

            quote = QuoteTick(
                instrument_id=instrument_id,
                bid_price=(
                    Price.from_str(msg.data.bid1Price)
                    if msg.data.bid1Price or last_quote is None
                    else last_quote.bid_price
                ),
                ask_price=(
                    Price.from_str(msg.data.ask1Price)
                    if msg.data.ask1Price or last_quote is None
                    else last_quote.ask_price
                ),
                bid_size=(
                    Quantity.from_str(msg.data.bid1Size)
                    if msg.data.bid1Size or last_quote is None
                    else last_quote.bid_size
                ),
                ask_size=(
                    Quantity.from_str(msg.data.ask1Size)
                    if msg.data.ask1Size or last_quote is None
                    else last_quote.ask_size
                ),
                ts_event=millis_to_nanos(msg.ts),
                ts_init=self._clock.timestamp_ns(),
            )

            self._last_quotes[quote.instrument_id] = quote
            self._handle_data(quote)
        except Exception as e:
            self._log.error(f"Failed to parse ticker: {msg} with error {e}")

    def _handle_trade(self, instrument_type: BybitInstrumentType, raw: bytes) -> None:
        msg = self._decoders["trade"].decode(raw)
        try:
            for data in msg.data:
                symbol = data.s + f"-{instrument_type.value.upper()}"
                instrument_id: InstrumentId = self._get_cached_instrument_id(symbol)
                trade: TradeTick = data.parse_to_trade_tick(
                    instrument_id,
                    self._clock.timestamp_ns(),
                )
                self._handle_data(trade)
        except Exception as e:
            self._log.error(f"Failed to parse trade tick: {msg} with error {e}")

    def _handle_kline(self, raw: bytes) -> None:
        msg = self._decoders["kline"].decode(raw)
        try:
            bar_type = self._topic_bar_type.get(msg.topic)
            for data in msg.data:
                if not data.confirm:
                    continue  # Bar still building
                bar: Bar = data.parse_to_bar(
                    bar_type,
                    self._clock.timestamp_ns(),
                )
                self._handle_data(bar)
        except Exception as e:
            self._log.error(f"Failed to parse bar: {msg} with error {e}")
