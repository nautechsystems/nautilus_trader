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
from nautilus_trader.adapters.bybit.schemas.market.ticker import BybitTickerData
from nautilus_trader.adapters.bybit.schemas.symbol import BybitSymbol
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsMessageGeneral
from nautilus_trader.adapters.bybit.schemas.ws import decoder_ws_ticker
from nautilus_trader.adapters.bybit.schemas.ws import decoder_ws_trade
from nautilus_trader.adapters.bybit.utils import get_api_key
from nautilus_trader.adapters.bybit.utils import get_api_secret
from nautilus_trader.adapters.bybit.websocket.client import BybitWebsocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.providers import InstrumentProvider
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
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import Instrument


class BybitDataClient(LiveMarketDataClient):
    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BybitHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: InstrumentProvider,
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

            # web socket decoders
            self._decoders = {
                "trade": decoder_ws_trade(),
                "ticker": decoder_ws_ticker(instrument_type),
            }
            self._decoder_ws_msg_general = msgspec.json.Decoder(BybitWsMessageGeneral)

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
    ):
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

    def complete_fetch_tickers_task(self, request: Request):
        # extract symbol from metadat
        if "symbol" not in request.metadata:
            raise ValueError("Symbol not in request metadata")
        symbol = request.metadata["symbol"]
        if not isinstance(symbol, Symbol):
            raise ValueError(
                f"Parameter symbol in request metadata object is not of type Symbol, got {type(symbol)}",
            )
        bybit_symbol = BybitSymbol(symbol.value)
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
        self._log.info("Initializing websocket connections.")
        for instrument_type, ws_client in self._ws_clients.items():
            await ws_client.connect()
        self._log.info("Data client connected.")

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
                    f"{self._update_instrument_interval}s.",
                )
                await asyncio.sleep(self._update_instrument_interval)
                await self._instrument_provider.load_all_async()
                self._send_all_instruments_to_data_engine()
        except asyncio.CancelledError:
            self._log.debug("Canceled `update_instruments` task.")

    async def _subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        symbol = BybitSymbol(instrument_id.symbol.value)
        ws_client = self._ws_clients[symbol.instrument_type]
        await ws_client.subscribe_trades(symbol.raw_symbol)
        self._log.info(f"Subscribed to trade ticks for {instrument_id}.")

    # async def _subscribe_ticker(self, instrument_id: InstrumentId) -> None:
    #     symbol = BybitSymbol(instrument_id.symbol.value)
    #     ws_client = self._ws_clients[symbol.instrument_type]
    #     await ws_client.subscribe_tickers(symbol.raw_symbol)
    #     self._log.info(f"Subscribed to ticker for {instrument_id}.")

    def _handle_ws_message(self, instrument_type: BybitInstrumentType, raw: bytes) -> None:
        try:
            ws_message = self._decoder_ws_msg_general.decode(raw)
            if ws_message.success is False:
                self._log.error(f"Error in ws_message: {ws_message.ret_msg}")
                return
            ## check if there is topic, if not discard it
            if ws_message.topic:
                self._topic_check(instrument_type, ws_message.topic, raw)
        except Exception as e:
            decoded_raw = raw.decode("utf-8")
            raise RuntimeError(f"Unknown websocket message type: {decoded_raw}") from e

    def _handle_trade(self, instrument_type: BybitInstrumentType, raw: bytes) -> None:
        try:
            msg = self._decoders["trade"].decode(raw)
            for trade in msg.data:
                symbol = trade.s + f"-{instrument_type.value.upper()}"
                instrument_id: InstrumentId = self._get_cached_instrument_id(symbol)
                trade_tick: TradeTick = trade.parse_to_trade_tick(
                    instrument_id,
                    self._clock.timestamp_ns(),
                )
                self._handle_data(trade_tick)
        except Exception as e:
            print("error in handle trade", e)
            decoded_raw = raw.decode("utf-8")
            self._log.error(f"Failed to parse trade tick: {decoded_raw}")

    def _handle_ticker(self, instrument_type: BybitInstrumentType, raw: bytes) -> None:
        try:
            self._decoders["ticker"].decode(raw)
        except Exception:
            print("failed to parse ticker ", raw)

    def _topic_check(self, instrument_type: BybitInstrumentType, topic: str, raw: bytes) -> None:
        if "publicTrade" in topic:
            self._handle_trade(instrument_type, raw)
        elif "tickers" in topic:
            self._handle_ticker(instrument_type, raw)
        else:
            self._log.error(f"Unknown websocket message topic: {topic} in Bybit")

    def _get_cached_instrument_id(self, symbol: str) -> InstrumentId:
        # Parse instrument ID
        bybit_symbol = BybitSymbol(symbol)
        nautilus_instrument_id: InstrumentId = bybit_symbol.parse_as_nautilus()
        return nautilus_instrument_id

    async def _request_instrument(
        self,
        instrument_id: InstrumentId,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        if start is not None:
            self._log.warning(
                f"Requesting instrument {instrument_id} with specified `start` which has no effect.",
            )

        if end is not None:
            self._log.warning(
                f"Requesting instrument {instrument_id} with specified `end` which has no effect.",
            )

        instrument: Instrument | None = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find instrument for {instrument_id}.")
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
                f"Requesting instruments for {venue} with specified `start` which has no effect.",
            )

        if end is not None:
            self._log.warning(
                f"Requesting instruments for {venue} with specified `end` which has no effect.",
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
                f"only historical bars with EXTERNAL aggregation available from Bybit.",
            )
            return

        if not bar_type.spec.is_time_aggregated():
            self._log.error(
                f"Cannot request {bar_type}: only time bars are aggregated by Bybit.",
            )
            return

        if bar_type.spec.price_type != PriceType.LAST:
            self._log.error(
                f"Cannot request {bar_type}: "
                f"only historical bars for LAST price type available from Binance.",
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
            # TODO fixing instrument here so that mypy passes,need to determine how to get instrument type from bar
            instrument_type=BybitInstrumentType.SPOT,
            bar_type=bar_type,
            interval=bybit_interval,
            start=start_time_ms,
            end=end_time_ms,
            limit=limit,
            ts_init=self._clock.timestamp_ns(),
        )
        partial: Bar = bars.pop()
        self._handle_bars(bar_type, bars, partial, correlation_id)

    async def _disconnect(self) -> None:
        if self._update_instruments_task:
            self._log.debug("Cancelling `update_instruments` task.")
            self._update_instruments_task.cancel()
            self._update_instruments_task = None
        for instrument_type, ws_client in self._ws_clients.items():
            await ws_client.disconnect()

    async def _handle_ticker_data_request(self, symbol: Symbol, correlation_id: UUID4) -> None:
        bybit_symbol = BybitSymbol(symbol.value)
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

    async def _request(self, data_type: DataType, correlation_id: UUID4) -> None:
        if data_type.type == BybitTickerData:
            symbol = data_type.metadata["symbol"]
            await self._handle_ticker_data_request(symbol, correlation_id)
