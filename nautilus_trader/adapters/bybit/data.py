import asyncio
from typing import Optional

import msgspec.json
import pandas as pd

from nautilus_trader.adapters.bybit.common.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.adapters.bybit.common.enums import BybitEnumParser
from nautilus_trader.adapters.bybit.config import BybitDataClientConfig
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.http.market import BybitMarketHttpAPI
from nautilus_trader.adapters.bybit.schemas.common import BybitWsSubscriptionMsg
from nautilus_trader.adapters.bybit.schemas.symbol import BybitSymbol
from nautilus_trader.adapters.bybit.schemas.ws import decoder_ws_trade,decoder_ws_ticker
from nautilus_trader.adapters.bybit.websocket.client import BybitWebsocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.uuid import UUID4

from nautilus_trader.core.datetime import secs_to_millis
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import TradeTick, BarType, Bar
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.model.enums import PriceType


class BybitWsTopicCheck(msgspec.Struct):
    topic: str


class BybitDataClient(LiveMarketDataClient):
    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BybitHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        instrument_provider: InstrumentProvider,
        instrument_type: BybitInstrumentType,
        base_url_ws: str,
        config: BybitDataClientConfig,
    ) -> None:
        self._instrument_type = instrument_type
        self._enum_parser = BybitEnumParser()
        super().__init__(
            loop=loop,
            client_id=ClientId(BYBIT_VENUE.value),
            venue=BYBIT_VENUE,
            instrument_provider=instrument_provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )
        # hot cache
        self._instrument_ids: dict[str, InstrumentId] = {}

        # http API
        self._http_market = BybitMarketHttpAPI(
            client=client,
            clock=clock,
            instrument_type=instrument_type,
        )

        # websocket API
        self._ws_client = BybitWebsocketClient(
            clock=clock,
            logger=logger,
            handler=self._handle_ws_message,
            base_url=base_url_ws
        )

        self._update_instrument_interval: int = 60 * 60  # Once per hour (hardcode)
        self._update_instruments_task: Optional[asyncio.Task] = None

        # web socket decoders
        self._decoders = {
            "trade": decoder_ws_trade(),
            "ticker": decoder_ws_ticker(instrument_type)
        }
        self._decoder_ws_topic_check = msgspec.json.Decoder(BybitWsTopicCheck)
        self._decoder_ws_subscription = msgspec.json.Decoder(BybitWsSubscriptionMsg)
        # self._decoder_ws_trade = msgspec.json.Decoder(BybitWsTradeMessage)
        # self._decoder_ws_ticker = de

    async def _connect(self) -> None:
        self._log.info("Initializing instruments...")
        await self._instrument_provider.initialize()

        self._send_all_instruments_to_data_engine()
        self._update_instruments_task = self.create_task(self._update_instruments())
        self._log.info("Initializing websocket connection.")
        await self._ws_client.connect()
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
            self._log.debug("`update_instruments` task was canceled.")

    async def _subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        symbol = BybitSymbol(instrument_id.symbol.value)
        await self._ws_client.subscribe_trades(symbol)
        self._log.info(f"Subscribed to trade ticks for {instrument_id}.")

    async def _subscribe_ticker(self, instrument_id: InstrumentId) -> None:
        symbol = BybitSymbol(instrument_id.symbol.value)
        await self._ws_client.subscribe_tickers(symbol)
        self._log.info(f"Subscribed to ticker for {instrument_id}.")

    def _handle_ws_message(self, raw: bytes) -> None:
        try:
            ws_message = self._decoder_ws_topic_check.decode(raw)
            self._topic_check(ws_message.topic, raw)
        except Exception as e:
            try:
                ws_message = self._decoder_ws_subscription.decode(raw)
                if ws_message.success:
                    self._log.info("Success subscribing")
                else:
                    self._log.error("Failed to subscribe.")
            except Exception as e:
                raise RuntimeError(f"Unknown websocket message type: {raw}", e)

    def _handle_trade(self, raw: bytes) -> None:
        try:
            msg = self._decoders["trade"].decode(raw)
            for trade in msg.data:
                instrument_id: InstrumentId = self._get_cached_instrument_id(trade.s)
                trade_tick: TradeTick = trade.parse_to_trade_tick(
                    instrument_id,
                    self._clock.timestamp_ns(),
                )
                self._handle_data(trade_tick)
        except Exception as e:
            self._log.error(f"Failed to parse trade tick: {raw}", e)

    def _handle_ticker(self, raw: bytes)-> None:
        try:
            msg = self._decoders["ticker"].decode(raw)
        except Exception as e:
            print("failed to parse ticker ",raw)

    def _topic_check(self, topic: str, raw: bytes) -> None:
        if "publicTrade" in topic:
            self._handle_trade(raw)
        elif "tickers" in topic:
            self._handle_ticker(raw)
        else:
            self._log.error(f"Unknown websocket message topic: {topic} in Bybit")

    def _get_cached_instrument_id(self, symbol: str) -> InstrumentId:
        # Parse instrument ID
        binance_symbol = BybitSymbol(symbol)
        assert binance_symbol
        nautilus_symbol: str = binance_symbol.parse_as_nautilus(
            self._instrument_type,
        )
        instrument_id: Optional[InstrumentId] = self._instrument_ids.get(nautilus_symbol)
        if not instrument_id:
            instrument_id = InstrumentId(Symbol(nautilus_symbol), BYBIT_VENUE)
            self._instrument_ids[nautilus_symbol] = instrument_id
        return instrument_id


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
            bar_type=bar_type,
            interval=bybit_interval,
            start=start_time_ms,
            end=end_time_ms,
            limit=limit,
            ts_init=self._clock.timestamp_ns(),
        )
        partial: Bar = bars.pop()
        self._handle_bars(bar_type,bars,partial,correlation_id)


