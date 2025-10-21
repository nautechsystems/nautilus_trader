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
from nautilus_trader.common.secure import mask_api_key
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import ensure_pydatetime_utc
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeFundingRates
from nautilus_trader.data.messages import SubscribeIndexPrices
from nautilus_trader.data.messages import SubscribeInstrument
from nautilus_trader.data.messages import SubscribeInstruments
from nautilus_trader.data.messages import SubscribeMarkPrices
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeFundingRates
from nautilus_trader.data.messages import UnsubscribeIndexPrices
from nautilus_trader.data.messages import UnsubscribeMarkPrices
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.cancellation import DEFAULT_FUTURE_CANCELLATION_TIMEOUT
from nautilus_trader.live.cancellation import cancel_tasks_with_timeout
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import FundingRateUpdate
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import book_type_to_str
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.instruments import CryptoPerpetual
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
        self._log.info(f"{config.instrument_families=}", LogColor.BLUE)
        self._log.info(f"config.contract_types={contract_types}", LogColor.BLUE)
        self._log.info(f"{config.is_demo=}", LogColor.BLUE)
        self._log.info(f"{config.http_timeout_secs=}", LogColor.BLUE)
        self._log.info(f"{config.max_retries=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_initial_ms=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_max_ms=}", LogColor.BLUE)
        self._log.info(f"{config.update_instruments_interval_mins=}", LogColor.BLUE)
        self._log.info(f"{config.vip_level=}", LogColor.BLUE)

        # HTTP API
        self._http_client = client
        if self._http_client.api_key:
            masked_key = mask_api_key(self._http_client.api_key)
            self._log.info(f"REST API key {masked_key}", LogColor.BLUE)

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

        if config.vip_level is not None:
            self._ws_client.set_vip_level(config.vip_level)
            self._ws_business_client.set_vip_level(config.vip_level)

    @property
    def instrument_provider(self) -> OKXInstrumentProvider:
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
        self._log.info(f"Connected to public websocket {self._ws_client.url}", LogColor.BLUE)

        await self._ws_business_client.connect(
            instruments=instruments,
            callback=self._handle_msg,
        )

        # Wait for connection to be established
        await self._ws_business_client.wait_until_active(timeout_secs=30.0)
        self._log.info(
            f"Connected to business websocket {self._ws_business_client.url}",
            LogColor.BLUE,
        )
        self._log.info("OKX API key authenticated", LogColor.GREEN)

        for instrument_type in self._instrument_provider.instrument_types:
            await self._ws_client.subscribe_instruments(instrument_type)

    async def _disconnect(self) -> None:
        self._http_client.cancel_all_requests()

        # Delay to allow websocket to send any unsubscribe messages
        await asyncio.sleep(1.0)

        # Shutdown public websocket
        if not self._ws_client.is_closed():
            self._log.info("Disconnecting public websocket")

            await self._ws_client.close()

            self._log.info(
                f"Disconnected from {self._ws_client.url}",
                LogColor.BLUE,
            )

        # Shutdown business websocket
        if not self._ws_business_client.is_closed():
            self._log.info("Disconnecting business websocket")

            await self._ws_business_client.close()

            self._log.info(
                f"Disconnected from {self._ws_business_client.url}",
                LogColor.BLUE,
            )

        # Cancel any pending futures
        all_futures = self._ws_client_futures | self._ws_business_client_futures
        await cancel_tasks_with_timeout(
            all_futures,
            self._log,
            timeout_secs=DEFAULT_FUTURE_CANCELLATION_TIMEOUT,
        )

        self._ws_client_futures.clear()
        self._ws_business_client_futures.clear()

    def _cache_instruments(self) -> None:
        # Ensures instrument definitions are available for correct
        # price and size precisions when parsing responses
        instruments_pyo3 = self.instrument_provider.instruments_pyo3()
        for inst in instruments_pyo3:
            self._http_client.add_instrument(inst)

        self._log.debug("Cached instruments", LogColor.MAGENTA)

    def _send_all_instruments_to_data_engine(self) -> None:
        for currency in self._instrument_provider.currencies().values():
            self._cache.add_currency(currency)

        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    async def _subscribe_instruments(self, command: SubscribeInstruments) -> None:
        pass  # Automatically subscribes for instruments websocket channel

    async def _subscribe_instrument(self, command: SubscribeInstrument) -> None:
        pass  # Automatically subscribes for instruments websocket channel

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        if command.book_type != BookType.L2_MBP:
            self._log.warning(
                f"Book type {book_type_to_str(command.book_type)} not supported by OKX, skipping subscription",
            )
            return

        if command.depth not in (0, 50, 400):
            self._log.error(
                "Cannot subscribe to order book deltas: "
                f"invalid `depth`, was {command.depth}; "
                "valid depths are 0 (default 400), 50, or 400",
            )
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_book_with_depth(pyo3_instrument_id, command.depth)

    async def _subscribe_order_book_snapshots(self, command: SubscribeOrderBook) -> None:
        if command.book_type != BookType.L2_MBP:
            self._log.warning(
                f"Book type {book_type_to_str(command.book_type)} not supported by OKX, skipping subscription",
            )
            return

        if command.depth not in (0, 5):
            self._log.error(
                "Cannot subscribe to order book snapshots: "
                f"invalid `depth`, was {command.depth}; "
                "valid depths are 0 (default 5), or 5",
            )
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)

        await self._ws_client.subscribe_book_depth5(pyo3_instrument_id)

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

    async def _subscribe_funding_rates(self, command: SubscribeFundingRates) -> None:
        # Funding rates only apply to perpetual swaps
        instrument = self._instrument_provider.find(command.instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find instrument for {command.instrument_id}")
            return

        # Check if instrument is a perpetual swap
        if not isinstance(instrument, CryptoPerpetual):
            self._log.warning(
                f"Funding rates not applicable for {command.instrument_id} "
                f"(instrument type: {type(instrument).__name__}), skipping subscription",
            )
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_funding_rates(pyo3_instrument_id)

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        active_channels = self._ws_client.get_subscriptions(pyo3_instrument_id)

        tasks = []

        for channel in active_channels:
            if channel == "books":
                tasks.append(self._ws_client.unsubscribe_book(pyo3_instrument_id))
            elif channel == "books50-l2-tbt":
                tasks.append(self._ws_client.unsubscribe_book50_l2_tbt(pyo3_instrument_id))
            elif channel == "books-l2-tbt":
                tasks.append(self._ws_client.unsubscribe_book_l2_tbt(pyo3_instrument_id))

        if tasks:
            await asyncio.gather(*tasks)

    async def _unsubscribe_order_book_snapshots(self, command: UnsubscribeOrderBook) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        active_channels = self._ws_client.get_subscriptions(pyo3_instrument_id)

        if "books5" in active_channels:
            await self._ws_client.unsubscribe_book_depth5(pyo3_instrument_id)

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

    async def _unsubscribe_funding_rates(self, command: UnsubscribeFundingRates) -> None:
        # Funding rates only apply to perpetual swaps
        instrument = self._instrument_provider.find(command.instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find instrument for {command.instrument_id}")
            return

        # Check if instrument is a perpetual swap
        if not isinstance(instrument, CryptoPerpetual):
            self._log.warning(
                f"Funding rates not applicable for {command.instrument_id} "
                f"(instrument type: {type(instrument).__name__}), skipping unsubscription",
            )
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_funding_rates(pyo3_instrument_id)

    # -- REQUESTS ---------------------------------------------------------------------------------

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

        self._handle_instrument(
            instrument,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    async def _request_instruments(self, request: RequestInstruments) -> None:
        if request.start is not None:
            self._log.warning(
                f"Requesting instruments for {request.venue} with specified `start` which has no effect",
            )

        if request.end is not None:
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

        pyo3_trades = await self._http_client.request_trades(
            instrument_id=pyo3_instrument_id,
            start=ensure_pydatetime_utc(request.start),
            end=ensure_pydatetime_utc(request.end),
            limit=request.limit,
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

    async def _request_bars(self, request: RequestBars) -> None:
        self._log.debug(
            f"Requesting bars: bar_type={request.bar_type}, start={request.start}, "
            f"end={request.end}, limit={request.limit}",
        )

        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(request.bar_type))

        pyo3_bars = await self._http_client.request_bars(
            bar_type=pyo3_bar_type,
            start=ensure_pydatetime_utc(request.start),
            end=ensure_pydatetime_utc(request.end),
            limit=request.limit,
        )
        bars = Bar.from_pyo3_list(pyo3_bars)

        self._handle_bars(
            request.bar_type,
            bars,
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
                # to `Data` is still owned and managed by Rust.
                data = capsule_to_data(msg)
                self._handle_data(data)
            elif isinstance(msg, nautilus_pyo3.FundingRateUpdate):
                self._handle_data(FundingRateUpdate.from_pyo3(msg))
            else:
                self._log.error(f"Cannot handle message {msg}, not implemented")
        except Exception as e:
            self._log.exception("Error handling websocket message", e)
