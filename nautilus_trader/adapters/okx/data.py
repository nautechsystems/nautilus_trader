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
from typing import Any

from nautilus_trader.adapters.okx.config import OKXDataClientConfig
from nautilus_trader.adapters.okx.constants import OKX_VENUE
from nautilus_trader.adapters.okx.providers import OKXInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.datetime import ensure_pydatetime_utc
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeIndexPrices
from nautilus_trader.data.messages import SubscribeMarkPrices
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeIndexPrices
from nautilus_trader.data.messages import UnsubscribeMarkPrices
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.instruments import Instrument


class OKXDataClient(LiveMarketDataClient):
    """
    Provides a data client for the OKX centralized crypto exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.OKXHttpClient
        The OKX HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : OKXInstrumentProvider
        The instrument provider.
    config : OKXDataClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.OKXHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: OKXInstrumentProvider,
        config: OKXDataClientConfig,
        name: str | None,
    ) -> None:
        PyCondition.not_empty(config.instrument_types, "config.instrument_types")
        super().__init__(
            loop=loop,
            client_id=ClientId(name or OKX_VENUE.value),
            venue=OKX_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        self._instrument_provider: OKXInstrumentProvider = instrument_provider

        instrument_types = [i.name.upper() for i in config.instrument_types]
        contract_types = (
            [c.name.upper() for c in config.contract_types] if config.contract_types else None
        )

        # Configuration
        self._config = config
        self._log.info(f"config.instrument_types={instrument_types}", LogColor.BLUE)
        self._log.info(f"config.contract_types={contract_types}", LogColor.BLUE)
        self._log.info(f"{config.http_timeout_secs=}", LogColor.BLUE)

        # HTTP API
        self._http_client = client
        self._log.info(f"REST API key {self._http_client.api_key}", LogColor.BLUE)

        # WebSocket API (using public endpoint for market data - no auth needed)
        self._ws_client = nautilus_pyo3.OKXWebSocketClient(
            url=config.base_url_ws or nautilus_pyo3.get_okx_ws_url_public(config.is_demo),
            api_key=None,  # Public endpoints don't need authentication
            api_secret=None,
            api_passphrase=None,
        )
        self._ws_client_futures: set[asyncio.Future] = set()

        # WebSocket API for business data (bars/candlesticks)
        self._ws_business_client = nautilus_pyo3.OKXWebSocketClient(
            url=nautilus_pyo3.get_okx_ws_url_business(config.is_demo),
            api_key=config.api_key,  # Business endpoint requires authentication
            api_secret=config.api_secret,
            api_passphrase=config.api_passphrase,
        )
        self._ws_business_client_futures: set[asyncio.Future] = set()

    @property
    def okx_instrument_provider(self) -> OKXInstrumentProvider:
        return self._instrument_provider

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        self._cache_instruments()
        self._send_all_instruments_to_data_engine()

        # Connect public WebSocket client
        future = asyncio.ensure_future(
            self._ws_client.connect(
                instruments=self.okx_instrument_provider.instruments_pyo3(),
                callback=self._handle_msg,
            ),
        )
        self._ws_client_futures.add(future)
        self._log.info(f"Connected to public websocket {self._ws_client.url}", LogColor.BLUE)

        # Connect business WebSocket client
        business_future = asyncio.ensure_future(
            self._ws_business_client.connect(
                instruments=self.okx_instrument_provider.instruments_pyo3(),
                callback=self._handle_msg,
            ),
        )
        self._ws_business_client_futures.add(business_future)
        self._log.info(
            f"Connected to business websocket {self._ws_business_client.url}",
            LogColor.BLUE,
        )
        self._log.info("OKX API key authenticated", LogColor.GREEN)

        # Subscribe to instruments for updates
        for instrument_type in self._instrument_provider._instrument_types:
            await self._ws_client.subscribe_instruments(instrument_type)

    async def _disconnect(self) -> None:
        # Delay to allow websocket to send any unsubscribe messages
        await asyncio.sleep(1.0)

        # Shutdown public websocket
        if self._ws_client is not None and not self._ws_client.is_closed():
            self._log.info("Disconnecting public websocket")
            close_result = self._ws_client.close()
            if close_result is not None:
                await close_result
            self._log.info(
                f"Disconnected from public websocket {self._ws_client.url}",
                LogColor.BLUE,
            )

        # Shutdown business websocket
        if self._ws_business_client is not None and not self._ws_business_client.is_closed():
            self._log.info("Disconnecting business websocket")
            close_result = self._ws_business_client.close()
            if close_result is not None:
                await close_result
            self._log.info(
                f"Disconnected from business websocket {self._ws_business_client.url}",
                LogColor.BLUE,
            )

        # Cancel all client futures
        for future in self._ws_client_futures:
            if not future.done():
                future.cancel()

        for future in self._ws_business_client_futures:
            if not future.done():
                future.cancel()

    def _cache_instruments(self) -> None:
        # Ensures instrument definitions are available for correct
        # price and size precisions when parsing responses
        instruments_pyo3 = self.okx_instrument_provider.instruments_pyo3()
        for inst in instruments_pyo3:
            self._http_client.add_instrument(inst)

        self._log.debug("Cached instruments", LogColor.MAGENTA)

    def _cache_instrument(self, instrument: Instrument) -> None:
        self._instrument_provider.add(instrument)
        self._http_client.add_instrument(instrument)

        self._log.debug(f"Cached instrument {instrument.id}", LogColor.MAGENTA)

    def _send_all_instruments_to_data_engine(self) -> None:
        for currency in self._instrument_provider.currencies().values():
            self._cache.add_currency(currency)

        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        if command.book_type == BookType.L3_MBO:
            self._log.error(
                "Cannot subscribe to order book deltas: "
                "L3_MBO data is not published by OKX. "
                "Valid book types are L1_MBP, L2_MBP",
            )
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_order_book(pyo3_instrument_id)

    async def _subscribe_order_book_snapshots(self, command: SubscribeOrderBook) -> None:
        # Same logic as deltas
        await self._subscribe_order_book_deltas(command)

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_quotes(pyo3_instrument_id)

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_trades(pyo3_instrument_id, aggregated=False)

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(command.bar_type))
        await self._ws_business_client.subscribe_bars(pyo3_bar_type)

    async def _subscribe_mark_prices(self, command: SubscribeMarkPrices) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_mark_prices(pyo3_instrument_id)

    async def _subscribe_index_prices(self, command: SubscribeIndexPrices) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_index_prices(pyo3_instrument_id)

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_order_book(pyo3_instrument_id)

    async def _unsubscribe_order_book_snapshots(self, command: UnsubscribeOrderBook) -> None:
        # Same logic as deltas
        await self._unsubscribe_order_book_deltas(command)

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_quotes(pyo3_instrument_id)

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_trades(pyo3_instrument_id, aggregated=False)

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(command.bar_type))
        await self._ws_business_client.unsubscribe_bars(pyo3_bar_type)

    async def _unsubscribe_mark_prices(self, command: UnsubscribeMarkPrices) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_mark_prices(pyo3_instrument_id)

    async def _unsubscribe_index_prices(self, command: UnsubscribeIndexPrices) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_index_prices(pyo3_instrument_id)

    # -- REQUESTS ---------------------------------------------------------------------------------

    async def _request_instrument(self, request: RequestInstrument) -> None:
        # Check if start/end times are too far from current time
        now = self._clock.utc_now()
        now_ns = dt_to_unix_nanos(now)
        start_ns = dt_to_unix_nanos(request.start)
        end_ns = dt_to_unix_nanos(request.end)

        if abs(start_ns - now_ns) > 10_000_000:  # More than 10ms difference
            self._log.warning(
                f"Requesting instrument {request.instrument_id} with specified `start` which has no effect",
            )

        if abs(end_ns - now_ns) > 10_000_000:  # More than 10ms difference
            self._log.warning(
                f"Requesting instrument {request.instrument_id} with specified `end` which has no effect",
            )

        instrument: Instrument | None = self._instrument_provider.find(request.instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find instrument for {request.instrument_id}")
            return

        self._handle_instrument(
            instrument,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    async def _request_instruments(self, request: RequestInstruments) -> None:
        # Check if start/end times are too far from current time
        now = self._clock.utc_now()
        now_ns = dt_to_unix_nanos(now)
        start_ns = dt_to_unix_nanos(request.start)
        end_ns = dt_to_unix_nanos(request.end)

        if abs(start_ns - now_ns) > 10_000_000:  # More than 10ms difference
            self._log.warning(
                f"Requesting instruments for {request.venue} with specified `start` which has no effect",
            )

        if abs(end_ns - now_ns) > 10_000_000:  # More than 10ms difference
            self._log.warning(
                f"Requesting instruments for {request.venue} with specified `end` which has no effect",
            )

        instruments = self._instrument_provider.get_all()

        self._handle_instruments(
            request.venue,
            instruments,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    async def _request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        self._log.error(
            "Cannot request historical quotes: not published by OKX. Subscribe to "
            "quotes or L1_MBP order book.",
        )

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        if request.start is None or request.end is None:
            self._log.error(
                f"Cannot request historical trades for {request.instrument_id}: "
                "both start and end times are required",
            )
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(request.instrument_id.value)
        trades = await self._http_client.request_trades(
            instrument_id=pyo3_instrument_id,
            start=ensure_pydatetime_utc(request.start),
            end=ensure_pydatetime_utc(request.end),
            limit=request.limit,
        )

        self._handle_trade_ticks(trades, request.id, request.params)

    async def _request_bars(self, request: RequestBars) -> None:
        self._log.debug(
            f"Requesting bars: bar_type={request.bar_type}, start={request.start}, "
            f"end={request.end}, limit={request.limit}",
        )

        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(request.bar_type))

        # Forward exact parameters to Rust layer (PY-2)
        pyo3_bars = await self._http_client.request_bars(
            bar_type=pyo3_bar_type,
            start=ensure_pydatetime_utc(request.start),
            end=ensure_pydatetime_utc(request.end),
            limit=request.limit,
        )
        bars = Bar.from_pyo3_list(pyo3_bars)

        # Log summary (PY-4)
        now = self._clock.utc_now()
        chosen_endpoint = (
            "history" if request.start and (now - request.start).days > 100 else "regular"
        )
        self._log.debug(
            f"Bars request completed: bar_type={request.bar_type}, start={request.start}, "
            f"end={request.end}, limit={request.limit}, endpoint={chosen_endpoint}, rows={len(bars)}",
        )

        self._handle_bars(
            request.bar_type,
            bars,
            None,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    # -- WEBSOCKET HANDLERS -----------------------------------------------------------------------

    def _handle_msg(self, msg: Any) -> None:
        if isinstance(msg, nautilus_pyo3.OKXWebSocketError):
            self._log.error(repr(msg))

        try:
            if nautilus_pyo3.is_pycapsule(msg):
                # The capsule will fall out of scope at the end of this method,
                # and eventually be garbage collected. The contained pointer
                # to `Data` is still owned and managed by Rust
                data = capsule_to_data(msg)
                self._handle_data(data)
            else:
                self._log.error(f"Cannot handle message {msg}, not implemented")
                return

        except Exception as e:
            self._log.exception("Error handling websocket message", e)
