# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.deribit.config import DeribitDataClientConfig
from nautilus_trader.adapters.deribit.constants import DERIBIT_DATA_SESSION_NAME
from nautilus_trader.adapters.deribit.constants import DERIBIT_VENUE
from nautilus_trader.adapters.deribit.providers import DeribitInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.cache.transformers import transform_instrument_from_pyo3
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.secure import mask_api_key
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.datetime import ensure_pydatetime_utc
from nautilus_trader.core.nautilus_pyo3 import DeribitCurrency
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestOrderBookSnapshot
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
from nautilus_trader.data.messages import UnsubscribeInstrument
from nautilus_trader.data.messages import UnsubscribeInstruments
from nautilus_trader.data.messages import UnsubscribeMarkPrices
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.cancellation import DEFAULT_FUTURE_CANCELLATION_TIMEOUT
from nautilus_trader.live.cancellation import cancel_tasks_with_timeout
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import FundingRateUpdate
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.model.enums import book_type_to_str
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.instruments import Instrument


def _bar_spec_to_deribit_resolution(bar_type) -> str:
    """
    Convert a bar type specification to a Deribit resolution string.

    Maps bar specifications to the nearest supported Deribit resolution:
    - Minutes: 1, 3, 5, 10, 15, 30, 60, 120, 180, 360, 720
    - Daily: 1D

    Parameters
    ----------
    bar_type : BarType
        The bar type to convert.

    Returns
    -------
    str
        The Deribit resolution string (e.g., "1", "60", "1D").

    """
    spec = bar_type.spec
    step = spec.step
    aggregation = spec.aggregation

    # Handle day aggregation
    if aggregation == BarAggregation.DAY:
        return "1D"

    # Handle hour aggregation (convert to minutes)
    if aggregation == BarAggregation.HOUR:
        return _map_hour_step_to_resolution(step)

    # Handle minute aggregation
    if aggregation == BarAggregation.MINUTE:
        return _map_minute_step_to_resolution(step)

    # Unsupported aggregation, default to 1 minute
    return "1"


def _map_minute_step_to_resolution(step: int) -> str:
    """
    Map minute step to nearest Deribit resolution.
    """
    # Thresholds: (max_step, resolution_string)
    thresholds = [
        (1, "1"),
        (3, "3"),
        (5, "5"),
        (10, "10"),
        (15, "15"),
        (30, "30"),
        (60, "60"),
        (120, "120"),
        (180, "180"),
        (360, "360"),
        (720, "720"),
    ]
    for max_step, resolution in thresholds:
        if step <= max_step:
            return resolution
    return "1D"


def _map_hour_step_to_resolution(step: int) -> str:
    """
    Map hour step to nearest Deribit resolution (in minutes).
    """
    if step == 1:
        return "60"
    if step == 2:
        return "120"
    if step == 3:
        return "180"
    if step <= 6:
        return "360"
    if step <= 12:
        return "720"
    return "1D"


class DeribitDataClient(LiveMarketDataClient):
    """
    Provides a data client for the Deribit centralized crypto derivatives exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.DeribitHttpClient
        The Deribit HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : DeribitInstrumentProvider
        The instrument provider.
    config : DeribitDataClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.DeribitHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: DeribitInstrumentProvider,
        config: DeribitDataClientConfig,
        name: str | None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or DERIBIT_VENUE.value),
            venue=DERIBIT_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        self._instrument_provider: DeribitInstrumentProvider = instrument_provider

        instrument_kinds = (
            [k.name for k in config.instrument_kinds] if config.instrument_kinds else None
        )

        # Configuration
        self._config = config
        self._log.info(f"config.instrument_kinds={instrument_kinds}", LogColor.BLUE)
        self._log.info(f"{config.is_testnet=}", LogColor.BLUE)
        self._log.info(f"{config.http_timeout_secs=}", LogColor.BLUE)
        self._log.info(f"{config.max_retries=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_initial_ms=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_max_ms=}", LogColor.BLUE)
        self._log.info(f"{config.update_instruments_interval_mins=}", LogColor.BLUE)

        # HTTP API
        self._http_client = client
        if config.api_key:
            masked_key = mask_api_key(config.api_key)
            self._log.info(f"REST API key {masked_key}", LogColor.BLUE)

        # WebSocket API
        ws_url = config.base_url_ws or nautilus_pyo3.get_deribit_ws_url(config.is_testnet)
        self._ws_client = nautilus_pyo3.DeribitWebSocketClient(
            url=ws_url,
            api_key=config.api_key,
            api_secret=config.api_secret,
            heartbeat_interval=30,
            is_testnet=config.is_testnet,
        )
        self._ws_client_futures: set[asyncio.Future] = set()

    @property
    def instrument_provider(self) -> DeribitInstrumentProvider:
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
        self._log.info(f"Connected to WebSocket {self._ws_client.url}", LogColor.BLUE)

        # Authenticate if credentials are configured (required for raw streams)
        if self._ws_client.has_credentials():
            self._log.info("Authenticating WebSocket session for raw streams...")
            await self._ws_client.authenticate_session(DERIBIT_DATA_SESSION_NAME)
            self._log.info("WebSocket authenticated", LogColor.GREEN)

    async def _disconnect(self) -> None:
        # Delay to allow WebSocket to send any unsubscribe messages
        await asyncio.sleep(1.0)

        if not self._ws_client.is_closed():
            self._log.info("Disconnecting WebSocket")

            await self._ws_client.close()

            self._log.info(
                f"Disconnected from {self._ws_client.url}",
                LogColor.BLUE,
            )

        # Cancel any pending futures
        await cancel_tasks_with_timeout(
            self._ws_client_futures,
            self._log,
            timeout_secs=DEFAULT_FUTURE_CANCELLATION_TIMEOUT,
        )

        self._ws_client_futures.clear()

    def _cache_instruments(self) -> None:
        # Ensures instrument definitions are available for correct
        # price and size precisions when parsing responses
        instruments_pyo3 = self.instrument_provider.instruments_pyo3()
        for inst in instruments_pyo3:
            self._http_client.cache_instrument(inst)

        self._ws_client.cache_instruments(instruments_pyo3)

        self._log.debug("Cached instruments", LogColor.MAGENTA)

    def _send_all_instruments_to_data_engine(self) -> None:
        for currency in self._instrument_provider.currencies().values():
            self._cache.add_currency(currency)

        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        if command.book_type != BookType.L2_MBP:
            self._log.warning(
                f"Book type {book_type_to_str(command.book_type)} not supported by Deribit, skipping subscription",
            )
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_book(pyo3_instrument_id)

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_quotes(pyo3_instrument_id)

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_trades(pyo3_instrument_id)

    async def _subscribe_order_book_depth(self, command: SubscribeOrderBook) -> None:
        """
        Subscribe to OrderBookDepth10 data for an instrument.

        Uses the Deribit grouped book channel: `book.{instrument}.{group}.{depth}.{interval}`
        with depth=10 for OrderBookDepth10 compatibility.

        Parameters
        ----------
        command : SubscribeOrderBook
            The subscription command containing instrument_id.

        """
        if command.book_type != BookType.L2_MBP:
            self._log.warning(
                f"Book type {book_type_to_str(command.book_type)} not supported by Deribit, skipping subscription",
            )
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)

        # OrderBookDepth10 uses depth=10 by default, but can be overridden
        depth = command.depth or 10
        if depth not in (1, 10, 20):
            if depth < 5:
                depth = 1
            elif depth < 15:
                depth = 10
            else:
                depth = 20

        # Use default grouping (no aggregation) and 100ms interval
        group = "none"
        interval = None

        self._log.info(
            f"Subscribing to order book depth for {command.instrument_id} "
            f"(depth={depth}, group={group}, interval=100ms)",
        )
        await self._ws_client.subscribe_book_grouped(pyo3_instrument_id, group, depth, interval)

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_book(pyo3_instrument_id)

    async def _unsubscribe_order_book_depth(self, command: UnsubscribeOrderBook) -> None:
        """
        Unsubscribe from OrderBookDepth10 data for an instrument.

        Parameters
        ----------
        command : UnsubscribeOrderBook
            The unsubscription command containing instrument_id.

        """
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)

        # Use default depth=10 for OrderBookDepth10
        depth = 10
        group = "none"
        interval = None

        self._log.info(
            f"Unsubscribing from order book depth for {command.instrument_id} "
            f"(depth={depth}, group={group}, interval=100ms)",
        )
        await self._ws_client.unsubscribe_book_grouped(pyo3_instrument_id, group, depth, interval)

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_quotes(pyo3_instrument_id)

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_trades(pyo3_instrument_id)

    async def _subscribe_instruments(self, command: SubscribeInstruments) -> None:
        """
        Subscribe to instrument state changes for all instruments.

        Uses the Deribit `instrument.state.{kind}.{currency}` WebSocket channel.

        Parameters in command.params:
        - kind: Instrument kind ("future", "option", "spot", etc.) - defaults to "any"
        - currency: Currency ("BTC", "ETH", "USDC", etc.) - defaults to "any"

        """
        kind = "any"
        currency = "any"

        if command.params:
            kind = command.params.get("kind", "any")
            currency = command.params.get("currency", "any")

        self._log.info(f"Subscribing to instrument state changes: {kind}.{currency}")
        await self._ws_client.subscribe_instrument_state(kind, currency)

    async def _subscribe_instrument(self, command: SubscribeInstrument) -> None:
        """
        Subscribe to instrument state changes for a specific instrument.

        Determines the kind and currency from the instrument ID and subscribes to the
        appropriate `instrument.state.{kind}.{currency}` channel.

        """
        symbol = command.instrument_id.symbol.value

        # Determine kind from instrument name pattern
        if "PERPETUAL" in symbol:
            kind = "future"
        elif symbol.endswith(("-C", "-P")):
            kind = "option"
        elif "_" in symbol and "-" not in symbol:
            kind = "spot"
        else:
            kind = "future"  # Default for futures with expiry dates like "BTC-28MAR25"

        # Extract currency from symbol
        # For instruments like "BTC-PERPETUAL", "BTC-28MAR25", "BTC_USDC"
        parts = symbol.replace("_", "-").split("-")
        currency = parts[0] if parts else "any"

        self._log.info(
            f"Subscribing to instrument state for {command.instrument_id} "
            f"(channel: instrument.state.{kind}.{currency})",
        )
        await self._ws_client.subscribe_instrument_state(kind, currency)

    async def _unsubscribe_instruments(self, command: UnsubscribeInstruments) -> None:
        """
        Unsubscribe from instrument state changes.

        Parameters in command.params:
        - kind: Instrument kind ("future", "option", "spot", etc.) - defaults to "any"
        - currency: Currency ("BTC", "ETH", "USDC", etc.) - defaults to "any"

        """
        kind = "any"
        currency = "any"

        if command.params:
            kind = command.params.get("kind", "any")
            currency = command.params.get("currency", "any")

        self._log.info(f"Unsubscribing from instrument state changes: {kind}.{currency}")
        await self._ws_client.unsubscribe_instrument_state(kind, currency)

    async def _unsubscribe_instrument(self, command: UnsubscribeInstrument) -> None:
        """
        Unsubscribe from instrument state changes for a specific instrument.

        Determines the kind and currency from the instrument ID and unsubscribes from
        the appropriate `instrument.state.{kind}.{currency}` channel.

        """
        symbol = command.instrument_id.symbol.value

        # Determine kind from instrument name pattern
        if "PERPETUAL" in symbol:
            kind = "future"
        elif symbol.endswith(("-C", "-P")):
            kind = "option"
        elif "_" in symbol and "-" not in symbol:
            kind = "spot"
        else:
            kind = "future"

        # Extract currency from symbol
        parts = symbol.replace("_", "-").split("-")
        currency = parts[0] if parts else "any"

        self._log.info(
            f"Unsubscribing from instrument state for {command.instrument_id} "
            f"(channel: instrument.state.{kind}.{currency})",
        )
        await self._ws_client.unsubscribe_instrument_state(kind, currency)

    async def _subscribe_mark_prices(self, command: SubscribeMarkPrices) -> None:
        """
        Subscribe to mark price updates for an instrument.

        Uses the Deribit ticker channel which provides mark price information.

        Parameters in command.params:
        - interval: Update interval (e.g., "100ms", "raw"). Defaults to 100ms.

        """
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)

        # Extract interval from params if provided
        interval = None
        if command.params:
            interval_str = command.params.get("interval")
            if interval_str:
                interval = nautilus_pyo3.DeribitUpdateInterval.from_str(interval_str)

        interval_display = interval.name if interval else "100ms (default)"
        self._log.info(
            f"Subscribing to mark prices for {command.instrument_id} "
            f"(via ticker channel, interval: {interval_display})",
        )
        await self._ws_client.subscribe_ticker(pyo3_instrument_id, interval)

    async def _subscribe_index_prices(self, command: SubscribeIndexPrices) -> None:
        """
        Subscribe to index price updates for an instrument.

        Uses the Deribit ticker channel which provides index price information.

        Parameters in command.params:
        - interval: Update interval (e.g., "100ms", "raw"). Defaults to 100ms.

        """
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)

        # Extract interval from params if provided
        interval = None
        if command.params:
            interval_str = command.params.get("interval")
            if interval_str:
                interval = nautilus_pyo3.DeribitUpdateInterval.from_str(interval_str)

        interval_display = interval.name if interval else "100ms (default)"
        self._log.info(
            f"Subscribing to index prices for {command.instrument_id} "
            f"(via ticker channel, interval: {interval_display})",
        )
        await self._ws_client.subscribe_ticker(pyo3_instrument_id, interval)

    async def _subscribe_funding_rates(self, command: SubscribeFundingRates) -> None:
        """
        Subscribe to funding rate updates for a perpetual instrument.

        Uses the Deribit perpetual channel which provides funding rate information.
        Only valid for perpetual instruments.

        Parameters in command.params:
        - interval: Update interval (e.g., "100ms", "raw"). Defaults to 100ms.

        """
        symbol = command.instrument_id.symbol.value

        # Validate instrument is a perpetual
        if "PERPETUAL" not in symbol:
            self._log.warning(
                f"Funding rates subscription rejected for {command.instrument_id}: "
                "only available for perpetual instruments",
            )
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)

        # Extract interval from params if provided
        interval = None
        if command.params:
            interval_str = command.params.get("interval")
            if interval_str:
                interval = nautilus_pyo3.DeribitUpdateInterval.from_str(interval_str)

        interval_display = interval.name if interval else "100ms (default)"
        self._log.info(
            f"Subscribing to funding rates for {command.instrument_id} "
            f"(via perpetual channel, interval: {interval_display})",
        )
        await self._ws_client.subscribe_perpetual_interest_rates(pyo3_instrument_id, interval)

    async def _unsubscribe_mark_prices(self, command: UnsubscribeMarkPrices) -> None:
        """
        Unsubscribe from mark price updates for an instrument.

        Parameters in command.params:
        - interval: Update interval (e.g., "100ms", "raw"). Defaults to 100ms.

        """
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)

        # Extract interval from params if provided
        interval = None
        if command.params:
            interval_str = command.params.get("interval")
            if interval_str:
                interval = nautilus_pyo3.DeribitUpdateInterval.from_str(interval_str)

        interval_display = interval.name if interval else "100ms (default)"
        self._log.info(
            f"Unsubscribing from mark prices for {command.instrument_id} "
            f"(via ticker channel, interval: {interval_display})",
        )
        await self._ws_client.unsubscribe_ticker(pyo3_instrument_id, interval)

    async def _unsubscribe_index_prices(self, command: UnsubscribeIndexPrices) -> None:
        """
        Unsubscribe from index price updates for an instrument.

        Parameters in command.params:
        - interval: Update interval (e.g., "100ms", "raw"). Defaults to 100ms.

        """
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)

        # Extract interval from params if provided
        interval = None
        if command.params:
            interval_str = command.params.get("interval")
            if interval_str:
                interval = nautilus_pyo3.DeribitUpdateInterval.from_str(interval_str)

        interval_display = interval.name if interval else "100ms (default)"
        self._log.info(
            f"Unsubscribing from index prices for {command.instrument_id} "
            f"(via ticker channel, interval: {interval_display})",
        )
        await self._ws_client.unsubscribe_ticker(pyo3_instrument_id, interval)

    async def _unsubscribe_funding_rates(self, command: UnsubscribeFundingRates) -> None:
        """
        Unsubscribe from funding rate updates for a perpetual instrument.

        Parameters in command.params:
        - interval: Update interval (e.g., "100ms", "raw"). Defaults to 100ms.

        """
        symbol = command.instrument_id.symbol.value

        # Validate instrument is a perpetual
        if "PERPETUAL" not in symbol:
            self._log.warning(
                f"Funding rates unsubscription rejected for {command.instrument_id}: "
                "only available for perpetual instruments",
            )
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)

        # Extract interval from params if provided
        interval = None
        if command.params:
            interval_str = command.params.get("interval")
            if interval_str:
                interval = nautilus_pyo3.DeribitUpdateInterval.from_str(interval_str)

        interval_display = interval.name if interval else "100ms (default)"
        self._log.info(
            f"Unsubscribing from funding rates for {command.instrument_id} "
            f"(via perpetual channel, interval: {interval_display})",
        )
        await self._ws_client.unsubscribe_perpetual_interest_rates(pyo3_instrument_id, interval)

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
            command.bar_type.instrument_id.value,
        )
        # Convert bar specification to Deribit resolution
        resolution = _bar_spec_to_deribit_resolution(command.bar_type)

        self._log.info(
            f"Subscribing to bars for {command.bar_type.instrument_id} (resolution: {resolution})",
            LogColor.BLUE,
        )
        await self._ws_client.subscribe_chart(pyo3_instrument_id, resolution)

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
            command.bar_type.instrument_id.value,
        )
        # Convert bar specification to Deribit resolution
        resolution = _bar_spec_to_deribit_resolution(command.bar_type)

        self._log.info(
            f"Unsubscribing from bars for {command.bar_type.instrument_id} "
            f"(resolution: {resolution})",
            LogColor.BLUE,
        )
        await self._ws_client.unsubscribe_chart(pyo3_instrument_id, resolution)

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

        try:
            pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(request.instrument_id.value)
            pyo3_instrument = await self._http_client.request_instrument(pyo3_instrument_id)
            self._cache_instrument(pyo3_instrument)
            instrument = transform_instrument_from_pyo3(pyo3_instrument)
        except Exception as e:
            self._log.error(f"Failed to request instrument {request.instrument_id}: {e}")
            return

        self._handle_instrument(
            instrument,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    async def _fetch_instruments_for_currency(
        self,
        currency: nautilus_pyo3.DeribitCurrency,
        kind: nautilus_pyo3.DeribitInstrumentKind | None = None,
    ) -> list[Instrument]:
        try:
            pyo3_instruments = await self._http_client.request_instruments(currency, kind)
            instruments = []
            for pyo3_instrument in pyo3_instruments:
                self._cache_instrument(pyo3_instrument)
                instrument = transform_instrument_from_pyo3(pyo3_instrument)
                instruments.append(instrument)
            return instruments
        except Exception as e:
            kind_str = f" kind {kind}" if kind else ""
            self._log.error(f"Failed to fetch instruments for {currency}{kind_str}: {e}")
            return []

    async def _request_instruments(self, request: RequestInstruments) -> None:
        if request.start is not None:
            self._log.warning(
                f"Requesting instruments for {request.venue} with specified `start` which has no effect",
            )

        if request.end is not None:
            self._log.warning(
                f"Requesting instruments for {request.venue} with specified `end` which has no effect",
            )

        all_instruments: list[Instrument] = []

        instrument_kinds = self._config.instrument_kinds

        if instrument_kinds:
            for kind in instrument_kinds:
                instruments = await self._fetch_instruments_for_currency(DeribitCurrency.ANY, kind)
                all_instruments.extend(instruments)
        else:
            instruments = await self._fetch_instruments_for_currency(
                DeribitCurrency.ANY,
                nautilus_pyo3.DeribitInstrumentKind.FUTURE,
            )
            all_instruments.extend(instruments)

        self._handle_instruments(
            request.venue,
            all_instruments,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(request.instrument_id.value)

        try:
            pyo3_trades = await self._http_client.request_trades(
                instrument_id=pyo3_instrument_id,
                start=ensure_pydatetime_utc(request.start),
                end=ensure_pydatetime_utc(request.end),
                limit=request.limit or None,
            )
        except Exception as e:
            self._log.exception(
                f"Failed to request trades for {request.instrument_id}",
                e,
            )
            return

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
        bar_type = request.bar_type
        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(bar_type))

        try:
            pyo3_bars = await self._http_client.request_bars(
                bar_type=pyo3_bar_type,
                start=ensure_pydatetime_utc(request.start),
                end=ensure_pydatetime_utc(request.end),
                limit=request.limit or None,
            )
        except Exception as e:
            self._log.exception(f"Failed to request bars for {bar_type}", e)
            return

        bars = Bar.from_pyo3_list(pyo3_bars)

        self._handle_bars(
            bar_type,
            bars,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    async def _request_order_book_snapshot(self, request: RequestOrderBookSnapshot) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(request.instrument_id.value)
        depth = request.limit if request.limit else None
        try:
            pyo3_book = await self._http_client.request_book_snapshot(
                instrument_id=pyo3_instrument_id,
                depth=depth,
            )
        except Exception as e:
            self._log.exception(
                f"Failed to request book snapshot for {request.instrument_id}",
                e,
            )
            return

        instrument = self._cache.instrument(request.instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find instrument for {request.instrument_id}")
            return

        ts_event = pyo3_book.ts_last
        ts_init = self._clock.timestamp_ns()
        sequence = pyo3_book.sequence

        # Build OrderBookDeltas from pyo3 book data
        deltas: list[OrderBookDelta] = []
        deltas.append(OrderBookDelta.clear(request.instrument_id, sequence, ts_event, ts_init))

        bids = list(pyo3_book.bids())
        asks = list(pyo3_book.asks())

        for i, level in enumerate(bids):
            order = BookOrder(
                side=OrderSide.BUY,
                price=instrument.make_price(level.price.as_double()),
                size=instrument.make_qty(level.size()),
                order_id=i,
            )
            delta = OrderBookDelta(
                instrument_id=request.instrument_id,
                action=BookAction.ADD,
                order=order,
                flags=0,
                sequence=sequence,
                ts_event=ts_event,
                ts_init=ts_init,
            )
            deltas.append(delta)

        for i, level in enumerate(asks):
            order = BookOrder(
                side=OrderSide.SELL,
                price=instrument.make_price(level.price.as_double()),
                size=instrument.make_qty(level.size()),
                order_id=len(bids) + i,
            )
            delta = OrderBookDelta(
                instrument_id=request.instrument_id,
                action=BookAction.ADD,
                order=order,
                flags=0,
                sequence=sequence,
                ts_event=ts_event,
                ts_init=ts_init,
            )
            deltas.append(delta)

        # Set F_LAST flag on the actual last delta (after CLEAR + bids + asks)
        if deltas:
            last = deltas[-1]
            deltas[-1] = OrderBookDelta(
                instrument_id=last.instrument_id,
                action=last.action,
                order=last.order,
                flags=RecordFlag.F_LAST,
                sequence=last.sequence,
                ts_event=last.ts_event,
                ts_init=last.ts_init,
            )

        snapshot = OrderBookDeltas(instrument_id=request.instrument_id, deltas=deltas)

        data_type = DataType(
            OrderBookDeltas,
            metadata={"instrument_id": request.instrument_id},
        )
        self._handle_data_response(
            data_type=data_type,
            data=[snapshot],
            correlation_id=request.id,
            start=None,
            end=None,
            params=request.params,
        )

    # -- WEBSOCKET HANDLERS -----------------------------------------------------------------------

    def _handle_msg(self, msg: Any) -> None:
        try:
            if nautilus_pyo3.is_pycapsule(msg):
                # The capsule will fall out of scope at the end of this method,
                # and eventually be garbage collected. The contained pointer
                # to `Data` is still owned and managed by Rust.
                data = capsule_to_data(msg)
                self._handle_data(data)
            elif hasattr(msg, "__class__") and "Instrument" in msg.__class__.__name__:
                self._handle_instrument_update(msg)
            elif hasattr(msg, "__class__") and "FundingRateUpdate" in msg.__class__.__name__:
                self._handle_funding_rate_update(msg)
            else:
                self._log.error(f"Cannot handle message {msg}, not implemented")
        except Exception as e:
            self._log.exception("Error handling WebSocket message", e)

    def _cache_instrument(self, pyo3_instrument: Any) -> None:
        self._http_client.cache_instrument(pyo3_instrument)

        if self._ws_client is not None:
            self._ws_client.cache_instrument(pyo3_instrument)

    def _handle_instrument_update(self, pyo3_instrument: Any) -> None:
        self._cache_instrument(pyo3_instrument)

        instrument = transform_instrument_from_pyo3(pyo3_instrument)

        self._handle_data(instrument)

    def _handle_funding_rate_update(self, pyo3_funding_rate: Any) -> None:
        data = FundingRateUpdate.from_pyo3(pyo3_funding_rate)
        self._handle_data(data)
