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
from collections import defaultdict
from decimal import Decimal
from functools import partial
from typing import TYPE_CHECKING, Any

import msgspec

from nautilus_trader.adapters.bybit.common.constants import BYBIT_INVERSE_DEPTHS
from nautilus_trader.adapters.bybit.common.constants import BYBIT_LINEAR_DEPTHS
from nautilus_trader.adapters.bybit.common.constants import BYBIT_OPTION_DEPTHS
from nautilus_trader.adapters.bybit.common.constants import BYBIT_SPOT_DEPTHS
from nautilus_trader.adapters.bybit.common.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.common.credentials import get_api_key
from nautilus_trader.adapters.bybit.common.credentials import get_api_secret
from nautilus_trader.adapters.bybit.common.enums import BybitEnumParser
from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.common.parsing import get_interval_from_bar_type
from nautilus_trader.adapters.bybit.common.symbol import BybitSymbol
from nautilus_trader.adapters.bybit.http.market import BybitMarketHttpAPI
from nautilus_trader.adapters.bybit.schemas.common import BYBIT_PONG
from nautilus_trader.adapters.bybit.schemas.market.ticker import BybitTickerData
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsMessageGeneral
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsTickerLinearMsg
from nautilus_trader.adapters.bybit.schemas.ws import decoder_ws_kline
from nautilus_trader.adapters.bybit.schemas.ws import decoder_ws_orderbook
from nautilus_trader.adapters.bybit.schemas.ws import decoder_ws_trade
from nautilus_trader.adapters.bybit.websocket.client import BybitWebSocketClient
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.datetime import secs_to_millis
from nautilus_trader.core.nautilus_pyo3 import Symbol
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.messages import DataResponse
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestData
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeFundingRates
from nautilus_trader.data.messages import SubscribeInstrument
from nautilus_trader.data.messages import SubscribeInstruments
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeFundingRates
from nautilus_trader.data.messages import UnsubscribeInstrument
from nautilus_trader.data.messages import UnsubscribeInstruments
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import CustomData
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import FundingRateUpdate
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId


if TYPE_CHECKING:
    from nautilus_trader.adapters.bybit.config import BybitDataClientConfig
    from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
    from nautilus_trader.adapters.bybit.providers import BybitInstrumentProvider
    from nautilus_trader.cache.cache import Cache
    from nautilus_trader.common.component import LiveClock
    from nautilus_trader.common.component import MessageBus
    from nautilus_trader.core.message import Request
    from nautilus_trader.model.instruments import Instrument


class BybitDataClient(LiveMarketDataClient):
    """
    Provides a data client for the Bybit centralized cypto exchange.

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
    product_types : list[BybitProductType]
        The product types for the client.
    ws_base_urls: dict[BybitProductType, str]
        The product base urls for the WebSocket clients.
    config : BybitDataClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    Notes
    -----
    Funding rate updates are automatically generated when subscribing to quote ticks
    for LINEAR and INVERSE perpetual swap instruments. The ticker WebSocket stream
    includes funding rate data which is parsed into FundingRateUpdate messages.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BybitHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: BybitInstrumentProvider,
        product_types: list[BybitProductType],
        ws_base_urls: dict[BybitProductType, str],
        config: BybitDataClientConfig,
        name: str | None,
    ) -> None:
        self._enum_parser = BybitEnumParser()
        super().__init__(
            loop=loop,
            client_id=ClientId(name or BYBIT_VENUE.value),
            venue=BYBIT_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        # Configuration
        self._bars_timestamp_on_close = config.bars_timestamp_on_close
        self._log.info(f"Product types: {[p.value for p in product_types]}", LogColor.BLUE)
        self._log.info(f"{config.update_instruments_interval_mins=}", LogColor.BLUE)
        self._log.info(f"{config.recv_window_ms=:_}", LogColor.BLUE)
        self._log.info(f"{config.bars_timestamp_on_close=}", LogColor.BLUE)

        # HTTP API
        self._http_market = BybitMarketHttpAPI(
            client=client,
            clock=clock,
        )

        # WebSocket API
        self._ws_clients: dict[BybitProductType, BybitWebSocketClient] = {}
        self._decoders: dict[str, dict[BybitProductType, msgspec.json.Decoder]] = defaultdict(
            dict,
        )
        for product_type in set(product_types):
            self._ws_clients[product_type] = BybitWebSocketClient(
                clock=clock,
                handler=partial(self._handle_ws_message, product_type),
                handler_reconnect=None,
                base_url=ws_base_urls[product_type],
                api_key=config.api_key or get_api_key(config.demo, config.testnet),
                api_secret=config.api_secret or get_api_secret(config.demo, config.testnet),
                loop=loop,
            )

        # WebSocket decoders
        self._decoder_ws_orderbook = decoder_ws_orderbook()
        self._decoder_ws_trade = decoder_ws_trade()
        self._decoder_ws_kline = decoder_ws_kline()
        self._decoder_ws_ticker_linear = msgspec.json.Decoder(BybitWsTickerLinearMsg)
        self._decoder_ws_msg_general = msgspec.json.Decoder(BybitWsMessageGeneral)

        self._tob_quotes: set[InstrumentId] = set()
        self._depths: dict[InstrumentId, int] = {}
        self._topic_bar_type: dict[str, BarType] = {}
        self._subscribed_tickers: set[InstrumentId] = set()
        self._funding_rate_cache: dict[InstrumentId, FundingRateUpdate] = {}

        self._update_instruments_interval_mins: int | None = config.update_instruments_interval_mins
        self._update_instruments_task: asyncio.Task | None = None

        # Register custom endpoint for fetching tickers
        self._msgbus.register(
            endpoint="bybit.data.tickers",
            handler=self.complete_fetch_tickers_task,
        )

        # Hot caches
        self._instrument_ids: dict[str, InstrumentId] = {}
        self._last_quotes: dict[InstrumentId, QuoteTick] = {}

    async def fetch_send_tickers(
        self,
        id: UUID4,
        product_type: BybitProductType,
        symbol: str,
    ) -> None:
        tickers = await self._http_market.fetch_tickers(
            product_type=product_type,
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
            start=None,
            end=None,
            params=None,
        )
        self._msgbus.response(data)

    def complete_fetch_tickers_task(self, request: Request) -> None:
        # Extract symbol from metadata
        if "symbol" not in request.metadata:
            raise ValueError("Symbol not in request metadata")
        symbol = request.metadata["symbol"]
        if not isinstance(symbol, Symbol):
            raise ValueError(
                f"Parameter symbol in request metadata object is not of type Symbol, was {type(symbol)}",
            )
        bybit_symbol = BybitSymbol(symbol.value)
        self.create_task(
            self.fetch_send_tickers(
                request.id,
                bybit_symbol.product_type,
                bybit_symbol.raw_symbol,
            ),
        )

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        self._send_all_instruments_to_data_engine()

        if self._update_instruments_interval_mins:
            self._update_instruments_task = self.create_task(
                self._update_instruments(self._update_instruments_interval_mins),
            )

        for ws_client in self._ws_clients.values():
            await ws_client.connect()

    async def _disconnect(self) -> None:
        # Delay to allow websocket to send any unsubscribe messages
        await asyncio.sleep(1.0)

        if self._update_instruments_task:
            self._log.debug("Canceling task 'update_instruments'")
            self._update_instruments_task.cancel()
            self._update_instruments_task = None

        for ws_client in self._ws_clients.values():
            await ws_client.disconnect()

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

    async def _subscribe_instruments(self, command: SubscribeInstruments) -> None:
        """
        Subscribe to instruments updates.

        Parameters
        ----------
        command : SubscribeInstruments
            The command to subscribe to instruments.

        """
        self._log.info("Skipping subscribe_instruments, Bybit subscribes automatically")

    async def _subscribe_instrument(self, command: SubscribeInstrument) -> None:
        """
        Subscribe to instrument updates.

        Parameters
        ----------
        command : SubscribeInstrument
            The command to subscribe to instrument.

        """
        self._log.info("Skipping subscribe_instrument, Bybit subscribes automatically")

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
        self._log.info("Skipping unsubscribe_instruments, not applicable for Bybit")

    async def _unsubscribe_instrument(self, command: UnsubscribeInstrument) -> None:
        """
        Unsubscribe from instrument updates.

        Parameters
        ----------
        command : UnsubscribeInstrument
            The command to unsubscribe from instrument updates.

        """
        self._log.info("Skipping unsubscribe_instrument, not applicable for Bybit")

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        if command.book_type == BookType.L3_MBO:
            self._log.error(
                "Cannot subscribe to order book deltas: "
                "L3_MBO data is not published by Bybit. "
                "Valid book types are L1_MBP, L2_MBP",
            )
            return

        bybit_symbol = BybitSymbol(command.instrument_id.symbol.value)
        product_type = bybit_symbol.product_type

        # Validate depth
        match product_type:
            case BybitProductType.SPOT:
                depths_available = BYBIT_SPOT_DEPTHS
                depth = command.depth or BYBIT_SPOT_DEPTHS[-1]
            case BybitProductType.LINEAR:
                depths_available = BYBIT_LINEAR_DEPTHS
                depth = command.depth or BYBIT_LINEAR_DEPTHS[-1]
            case BybitProductType.INVERSE:
                depths_available = BYBIT_INVERSE_DEPTHS
                depth = command.depth or BYBIT_INVERSE_DEPTHS[-1]
            case BybitProductType.OPTION:
                depths_available = BYBIT_OPTION_DEPTHS
                depth = command.depth or BYBIT_OPTION_DEPTHS[-1]
            case _:
                # Theoretically unreachable but retained to keep the match exhaustive
                raise ValueError(
                    f"Invalid Bybit product type {product_type}",
                )

        if depth not in depths_available:
            self._log.error(
                f"Cannot subscribe to order book depth {depth} "
                f"for Bybit {product_type.value} products, "
                f"available depths are {depths_available}",
            )
            return

        if command.instrument_id in self._tob_quotes:
            if depth == 1:
                self._log.warning(
                    f"Already subscribed to {command.instrument_id} top-of-book",
                    LogColor.MAGENTA,
                )
                return  # Already subscribed

        if command.instrument_id in self._depths:
            self._log.warning(f"Already subscribed to {command.instrument_id} order book deltas")
            return

        self._depths[command.instrument_id] = depth
        ws_client = self._ws_clients[bybit_symbol.product_type]
        await ws_client.subscribe_order_book(bybit_symbol.raw_symbol, depth=depth)

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        bybit_symbol = BybitSymbol(command.instrument_id.symbol.value)
        ws_client = self._ws_clients[bybit_symbol.product_type]

        if bybit_symbol.is_spot or command.instrument_id not in self._depths:
            # Subscribe top level (faster 10ms updates)
            self._log.debug(
                f"Subscribing quotes {command.instrument_id} (faster top-of-book @10ms)",
                LogColor.MAGENTA,
            )
            self._tob_quotes.add(command.instrument_id)
            await ws_client.subscribe_order_book(bybit_symbol.raw_symbol, depth=1)
        else:
            # Subscribe to tickers (includes funding rate for perpetual swaps)
            # Note: For LINEAR and INVERSE perpetual swaps, this will also generate
            # FundingRateUpdate messages in addition to QuoteTicks
            if command.instrument_id not in self._subscribed_tickers:
                await ws_client.subscribe_tickers(bybit_symbol.raw_symbol)
                self._subscribed_tickers.add(command.instrument_id)

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        bybit_symbol = BybitSymbol(command.instrument_id.symbol.value)
        ws_client = self._ws_clients[bybit_symbol.product_type]
        await ws_client.subscribe_trades(bybit_symbol.raw_symbol)

    async def _subscribe_funding_rates(self, command: SubscribeFundingRates) -> None:
        bybit_symbol = BybitSymbol(command.instrument_id.symbol.value)

        # Only perpetual swaps have funding rates
        if bybit_symbol.product_type not in [BybitProductType.LINEAR, BybitProductType.INVERSE]:
            self._log.warning(
                f"Cannot subscribe to funding rates for {command.instrument_id} - "
                f"only LINEAR and INVERSE perpetual swaps support funding rates",
            )
            return

        # If we're not already subscribed to tickers, subscribe now
        if command.instrument_id not in self._subscribed_tickers:
            ws_client = self._ws_clients[bybit_symbol.product_type]
            await ws_client.subscribe_tickers(bybit_symbol.raw_symbol)
            self._subscribed_tickers.add(command.instrument_id)

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        bybit_symbol = BybitSymbol(command.bar_type.instrument_id.symbol.value)
        ws_client = self._ws_clients[bybit_symbol.product_type]
        interval_str = get_interval_from_bar_type(command.bar_type)
        topic = f"kline.{interval_str}.{bybit_symbol.raw_symbol}"
        self._topic_bar_type[topic] = command.bar_type
        await ws_client.subscribe_klines(bybit_symbol.raw_symbol, interval_str)

    async def _unsubscribe_funding_rates(self, command: UnsubscribeFundingRates) -> None:
        # Check if we can unsubscribe from tickers
        # (only if no other subscription needs them)
        # Need to check if quotes are subscribed via ticker (not TOB)
        quotes_via_ticker = (
            command.instrument_id in self._depths and command.instrument_id not in self._tob_quotes
        )
        if command.instrument_id in self._subscribed_tickers and not quotes_via_ticker:
            bybit_symbol = BybitSymbol(command.instrument_id.symbol.value)
            ws_client = self._ws_clients[bybit_symbol.product_type]
            await ws_client.unsubscribe_tickers(bybit_symbol.raw_symbol)
            self._subscribed_tickers.discard(command.instrument_id)
            self._log.debug(
                f"Unsubscribed from funding rates for {command.instrument_id}",
                LogColor.MAGENTA,
            )

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        bybit_symbol = BybitSymbol(command.instrument_id.symbol.value)
        ws_client = self._ws_clients[bybit_symbol.product_type]
        depth = self._depths.get(command.instrument_id, 1)
        await ws_client.unsubscribe_order_book(bybit_symbol.raw_symbol, depth=depth)
        self._depths.pop(command.instrument_id, None)

    async def _unsubscribe_order_book_snapshots(self, command: UnsubscribeOrderBook) -> None:
        bybit_symbol = BybitSymbol(command.instrument_id.symbol.value)
        ws_client = self._ws_clients[bybit_symbol.product_type]
        depth = self._depths.get(command.instrument_id, 1)
        await ws_client.unsubscribe_order_book(bybit_symbol.raw_symbol, depth=depth)
        self._depths.pop(command.instrument_id, None)

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        bybit_symbol = BybitSymbol(command.instrument_id.symbol.value)
        ws_client = self._ws_clients[bybit_symbol.product_type]
        if command.instrument_id in self._tob_quotes:
            await ws_client.unsubscribe_order_book(bybit_symbol.raw_symbol, depth=1)
            self._tob_quotes.discard(command.instrument_id)
        else:
            # Check if we can unsubscribe from tickers
            # (only if funding rates are not also subscribed)
            if (
                command.instrument_id in self._subscribed_tickers
                and command.instrument_id not in self._subscribed_funding_rates
            ):
                await ws_client.unsubscribe_tickers(bybit_symbol.raw_symbol)
                self._subscribed_tickers.discard(command.instrument_id)

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        bybit_symbol = BybitSymbol(command.instrument_id.symbol.value)
        ws_client = self._ws_clients[bybit_symbol.product_type]
        await ws_client.unsubscribe_trades(bybit_symbol.raw_symbol)

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        bybit_symbol = BybitSymbol(command.bar_type.instrument_id.symbol.value)
        ws_client = self._ws_clients[bybit_symbol.product_type]
        interval_str = get_interval_from_bar_type(command.bar_type)
        topic = f"kline.{interval_str}.{bybit_symbol.raw_symbol}"
        self._topic_bar_type.pop(topic, None)
        await ws_client.unsubscribe_klines(bybit_symbol.raw_symbol, interval_str)

    def _get_cached_instrument_id(
        self,
        symbol: str,
        product_type: BybitProductType,
    ) -> InstrumentId:
        bybit_symbol = BybitSymbol(f"{symbol}-{product_type.value.upper()}")
        return bybit_symbol.to_instrument_id()

    async def _request(self, request: RequestData) -> None:
        if request.data_type.type == BybitTickerData:
            symbol = request.data_type.metadata["symbol"]
            await self._handle_ticker_data_request(symbol, request.id)

    async def _request_instrument(self, request: RequestInstrument) -> None:
        if request.start is not None:
            self._log.warning(
                f"Requesting instrument {request.instrument_id} with specified `start` which has no effect",
            )

        if request.end is not None:
            self._log.warning(
                f"Requesting instrument {request.instrument_id} with specified `end` which has no effect",
            )

        instrument: Instrument | None = self._instrument_provider.find(request.instrument_id)
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
        self._log.error(
            "Cannot request historical quotes: not published by Bybit",
        )

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        limit = request.limit

        if limit == 0 or limit > 1000:
            limit = 1000

        # Check if request is for trades older than one day
        now = self._clock.utc_now()
        now_ns = dt_to_unix_nanos(now)
        start_ns = dt_to_unix_nanos(request.start)
        end_ns = dt_to_unix_nanos(request.end)
        one_day_ns = 86_400_000_000_000  # One day in nanoseconds

        if (now_ns - start_ns) > one_day_ns:
            self._log.error(
                "Cannot specify `start` older then 1 day for historical trades: Bybit only provides '1 day old trades'",
            )

        trades = await self._http_market.request_bybit_trades(
            instrument_id=request.instrument_id,
            limit=limit,
        )

        # Filter trades to only include those within the requested time range
        filtered_trades = [trade for trade in trades if start_ns <= trade.ts_init <= end_ns]

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

        bybit_interval = self._enum_parser.parse_bybit_kline(request.bar_type)
        start_time_ms = secs_to_millis(request.start.timestamp())
        end_time_ms = secs_to_millis(request.end.timestamp())

        self._log.debug(f"Requesting klines {start_time_ms=}, {end_time_ms=}, {request.limit=}")

        bars = await self._http_market.request_bybit_bars(
            bar_type=request.bar_type,
            interval=bybit_interval,
            start=start_time_ms,
            end=end_time_ms,
            limit=request.limit if request.limit else None,
            timestamp_on_close=self._bars_timestamp_on_close,
        )
        # For historical data requests, all bars are complete (no partial bars)
        self._handle_bars(
            request.bar_type,
            bars,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    async def _handle_ticker_data_request(self, symbol: Symbol, correlation_id: UUID4) -> None:
        bybit_symbol = BybitSymbol(symbol.value)
        bybit_tickers = await self._http_market.fetch_tickers(
            product_type=bybit_symbol.product_type,
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
            None,
            None,
            None,
        )

    def _handle_ws_message(self, product_type: BybitProductType, raw: bytes) -> None:
        try:
            ws_message = self._decoder_ws_msg_general.decode(raw)
            if ws_message.op == BYBIT_PONG:
                return
            if ws_message.success is False:
                self._log.error(f"WebSocket error: {ws_message}")
                return
            if not ws_message.topic:
                return

            if "orderbook" in ws_message.topic:
                self._handle_orderbook(product_type, raw, ws_message.topic)
            elif "publicTrade" in ws_message.topic:
                self._handle_trade(product_type, raw)
            elif "tickers" in ws_message.topic:
                self._handle_ticker(product_type, raw)
            elif "kline" in ws_message.topic:
                self._handle_kline(raw)
            else:
                self._log.error(f"Unknown websocket message topic: {ws_message.topic}")
        except Exception as e:
            self._log.exception(f"Failed to parse websocket message: {raw.decode()}", e)

    def _handle_orderbook(self, product_type: BybitProductType, raw: bytes, topic: str) -> None:
        msg = self._decoder_ws_orderbook.decode(raw)
        instrument_id = self._get_cached_instrument_id(msg.data.s, product_type)
        instrument = self._cache.instrument(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot parse order book data: no instrument for {instrument_id}")
            return

        if instrument_id in self._tob_quotes and topic.startswith("orderbook.1."):
            quote = msg.data.parse_to_quote_tick(
                instrument_id=instrument_id,
                last_quote=self._last_quotes.get(instrument_id),
                price_precision=instrument.price_precision,
                size_precision=instrument.size_precision,
                ts_event=millis_to_nanos(msg.ts),
                ts_init=self._clock.timestamp_ns(),
            )
            self._last_quotes[quote.instrument_id] = quote
            self._handle_data(quote)
            return

        if msg.type == "snapshot":
            deltas: OrderBookDeltas = msg.data.parse_to_deltas(
                instrument_id=instrument_id,
                price_precision=instrument.price_precision,
                size_precision=instrument.size_precision,
                ts_event=millis_to_nanos(msg.ts),
                ts_init=self._clock.timestamp_ns(),
                snapshot=True,
            )
        else:
            deltas = msg.data.parse_to_deltas(
                instrument_id=instrument_id,
                price_precision=instrument.price_precision,
                size_precision=instrument.size_precision,
                ts_event=millis_to_nanos(msg.ts),
                ts_init=self._clock.timestamp_ns(),
            )
        self._handle_data(deltas)

    def _handle_ticker(self, product_type: BybitProductType, raw: bytes) -> None:
        """
        Handle ticker data from Bybit websocket.
        """
        if product_type not in [
            BybitProductType.LINEAR,
            BybitProductType.INVERSE,
            BybitProductType.OPTION,
        ]:
            return

        # Parse ticker data
        ticker = self._parse_ticker_data(raw, product_type)
        if not ticker:
            return

        # Create quote tick
        quote_tick = self._create_quote_tick_from_ticker(ticker, product_type)
        if quote_tick:
            self._handle_data(quote_tick)

        # Create funding rate update for perpetual swaps (LINEAR and INVERSE only)
        # The framework will handle routing to subscribers
        if product_type in [BybitProductType.LINEAR, BybitProductType.INVERSE]:
            funding_rate_update = self._create_funding_rate_update_from_ticker(ticker, product_type)
            if funding_rate_update:
                # Check if we have a cached rate for this instrument
                cached_rate = self._funding_rate_cache.get(funding_rate_update.instrument_id)

                # Only emit if this is new or changed (uses custom __eq__ comparing rate and next_funding_ns)
                if cached_rate is None or cached_rate != funding_rate_update:
                    self._funding_rate_cache[funding_rate_update.instrument_id] = (
                        funding_rate_update
                    )
                    self._handle_data(funding_rate_update)

    def _parse_ticker_data(self, raw: bytes, product_type: BybitProductType) -> Any:
        """
        Parse ticker data from raw bytes.
        """
        try:
            # Use the appropriate decoder based on product type
            if product_type == BybitProductType.LINEAR:
                msg = self._decoder_ws_ticker_linear.decode(raw)
                return msg.data
            else:
                # For INVERSE and OPTION, use general decoder
                data = self._decoder_ws_msg_general.decode(raw)
                ticker_data = getattr(data, "data", None)
                if ticker_data is None:
                    return None

                # For ticker messages, the data field might be a list or single item
                if isinstance(ticker_data, list) and len(ticker_data) > 0:
                    return ticker_data[0]
                return ticker_data
        except Exception as e:
            self._log.error(f"Error parsing ticker data: {e}")
            return None

    def _create_quote_tick_from_ticker(
        self,
        ticker: Any,
        product_type: BybitProductType,
    ) -> QuoteTick | None:
        """
        Create QuoteTick from ticker data.
        """
        try:
            # Get the symbol and instrument
            symbol = getattr(ticker, "symbol", None)
            if not symbol:
                return None

            instrument_id = self._get_cached_instrument_id(symbol, product_type)
            instrument = self._cache.instrument(instrument_id)
            if instrument is None:
                self._log.error(f"Cannot create quote tick: no instrument for {instrument_id}")
                return None

            # Extract price and size data
            bid_price_str = getattr(ticker, "bid1Price", None)
            ask_price_str = getattr(ticker, "ask1Price", None)
            bid_size_str = getattr(ticker, "bid1Size", None)
            ask_size_str = getattr(ticker, "ask1Size", None)

            if not bid_price_str or not ask_price_str:
                return None

            # Create Price and Quantity objects
            from nautilus_trader.model.objects import Price
            from nautilus_trader.model.objects import Quantity

            bid_price = Price.from_str(bid_price_str)
            ask_price = Price.from_str(ask_price_str)
            bid_size = Quantity.from_str(bid_size_str) if bid_size_str else Quantity.from_int(0)
            ask_size = Quantity.from_str(ask_size_str) if ask_size_str else Quantity.from_int(0)

            # Get timestamp
            ts_event = millis_to_nanos(int(getattr(ticker, "ts", 0)))
            if ts_event == 0:
                ts_event = self._clock.timestamp_ns()

            # Create QuoteTick
            return QuoteTick(
                instrument_id=instrument_id,
                bid_price=bid_price,
                ask_price=ask_price,
                bid_size=bid_size,
                ask_size=ask_size,
                ts_event=ts_event,
                ts_init=self._clock.timestamp_ns(),
            )
        except Exception as e:
            self._log.error(f"Error creating QuoteTick from ticker: {e}")
            return None

    def _extract_ticker_prices_and_sizes(
        self,
        ticker: Any,
        product_type: BybitProductType,
    ) -> tuple[float | None, float | None, float | None, float | None]:
        """
        Extract bid/ask prices and sizes from ticker data.
        """
        if product_type == BybitProductType.LINEAR:
            return (
                getattr(ticker, "bid1Price", None),
                getattr(ticker, "ask1Price", None),
                getattr(ticker, "bid1Size", None),
                getattr(ticker, "ask1Size", None),
            )
        elif product_type == BybitProductType.INVERSE:
            return (
                getattr(ticker, "bid1Price", None),
                getattr(ticker, "ask1Price", None),
                getattr(ticker, "bid1Size", None),
                getattr(ticker, "ask1Size", None),
            )
        elif product_type == BybitProductType.OPTION:
            return (
                getattr(ticker, "bid1Price", None),
                getattr(ticker, "ask1Price", None),
                getattr(ticker, "bid1Size", None),
                getattr(ticker, "ask1Size", None),
            )
        return None, None, None, None

    def _get_instrument_id_from_ticker(self, ticker: Any) -> InstrumentId:
        """
        Get InstrumentId from ticker data.
        """
        symbol = getattr(ticker, "symbol", "")
        return InstrumentId.from_str(f"{symbol}.BYBIT")

    def _get_timestamp_from_ticker(self, ticker: Any) -> int:
        """
        Get timestamp from ticker data.
        """
        return getattr(ticker, "ts", self._clock.timestamp_ns())

    def _create_funding_rate_update_from_ticker(
        self,
        ticker: Any,
        product_type: BybitProductType,
    ) -> FundingRateUpdate | None:
        """
        Create FundingRateUpdate from ticker data for perpetual swaps.
        """
        try:
            # Get funding rate
            funding_rate_str = getattr(ticker, "fundingRate", None)
            if not funding_rate_str:
                return None

            # Parse funding rate and normalize to remove trailing zeros
            funding_rate = Decimal(funding_rate_str).normalize()

            # Get next funding time (milliseconds)
            next_funding_time_str = getattr(ticker, "nextFundingTime", None)
            next_funding_ns = None
            if next_funding_time_str:
                try:
                    # Bybit provides next funding time as a millisecond timestamp string
                    next_funding_ns = int(next_funding_time_str) * 1_000_000  # Convert ms to ns
                except (ValueError, TypeError):
                    self._log.warning(f"Failed to parse next funding time: {next_funding_time_str}")

            # Get instrument ID
            symbol = getattr(ticker, "symbol", None)
            if not symbol:
                return None

            instrument_id = self._get_cached_instrument_id(symbol, product_type)

            # Get timestamp from message
            ts_event = millis_to_nanos(int(getattr(ticker, "ts", 0)))
            if ts_event == 0:
                ts_event = self._clock.timestamp_ns()

            return FundingRateUpdate(
                instrument_id=instrument_id,
                rate=funding_rate,
                ts_event=ts_event,
                ts_init=self._clock.timestamp_ns(),
                next_funding_ns=next_funding_ns,
            )
        except Exception as e:
            self._log.error(f"Error creating FundingRateUpdate from ticker: {e}")
            return None

    def _handle_trade(self, product_type: BybitProductType, raw: bytes) -> None:
        msg = self._decoder_ws_trade.decode(raw)
        try:
            for data in msg.data:
                instrument_id = self._get_cached_instrument_id(data.s, product_type)
                instrument = self._cache.instrument(instrument_id)
                if instrument is None:
                    self._log.error(f"Cannot parse trade data: no instrument for {instrument_id}")
                    return

                trade: TradeTick = data.parse_to_trade_tick(
                    instrument_id,
                    price_precision=instrument.price_precision,
                    size_precision=instrument.size_precision,
                    ts_init=self._clock.timestamp_ns(),
                )
                self._handle_data(trade)
        except Exception as e:
            self._log.exception(f"Failed to parse trade tick: {msg}", e)

    def _handle_kline(self, raw: bytes) -> None:
        msg = self._decoder_ws_kline.decode(raw)
        try:
            bar_type = self._topic_bar_type.get(msg.topic)

            if bar_type is None:
                self._log.error(f"Cannot parse bar data: no bar_type for {msg.topic}")
                return

            instrument_id = bar_type.instrument_id
            instrument = self._cache.instrument(instrument_id)

            if instrument is None:
                self._log.error(f"Cannot parse bar data: no instrument for {instrument_id}")
                return

            for data in msg.data:
                if not data.confirm:
                    continue  # Bar still building
                bar: Bar = data.parse_to_bar(
                    bar_type=bar_type,
                    price_precision=instrument.price_precision,
                    size_precision=instrument.size_precision,
                    ts_init=self._clock.timestamp_ns(),
                    timestamp_on_close=self._bars_timestamp_on_close,
                )
                self._handle_data(bar)
        except Exception as e:
            self._log.exception(f"Failed to parse bar: {msg}", e)
