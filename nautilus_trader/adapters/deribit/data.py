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

from nautilus_trader.adapters.deribit.config import DeribitDataClientConfig
from nautilus_trader.adapters.deribit.constants import DERIBIT_VENUE
from nautilus_trader.adapters.deribit.providers import DeribitInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.cache.transformers import transform_instrument_from_pyo3
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.secure import mask_api_key
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import DeribitCurrency
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.cancellation import DEFAULT_FUTURE_CANCELLATION_TIMEOUT
from nautilus_trader.live.cancellation import cancel_tasks_with_timeout
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import book_type_to_str
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.instruments import Instrument


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
        self._log.info(f"Connected to websocket {self._ws_client.url}", LogColor.BLUE)

    async def _disconnect(self) -> None:
        # Delay to allow websocket to send any unsubscribe messages
        await asyncio.sleep(1.0)

        # Shutdown websocket
        if not self._ws_client.is_closed():
            self._log.info("Disconnecting websocket")

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

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_book(pyo3_instrument_id)

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_quotes(pyo3_instrument_id)

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_trades(pyo3_instrument_id)

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
            self._handle_instrument_update(pyo3_instrument)
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
                self._handle_instrument_update(pyo3_instrument)
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

        instrument_kinds = self._instrument_provider.instrument_kinds

        if instrument_kinds:
            for kind in instrument_kinds:
                instruments = await self._fetch_instruments_for_currency(DeribitCurrency.ANY, kind)
                all_instruments.extend(instruments)
        else:
            instruments = await self._fetch_instruments_for_currency(DeribitCurrency.ANY)
            all_instruments.extend(instruments)

        self._handle_instruments(
            request.venue,
            all_instruments,
            request.id,
            request.start,
            request.end,
            request.params,
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
            else:
                self._log.error(f"Cannot handle message {msg}, not implemented")
        except Exception as e:
            self._log.exception("Error handling websocket message", e)

    def _handle_instrument_update(self, pyo3_instrument: Any) -> None:
        self._http_client.cache_instrument(pyo3_instrument)

        if self._ws_client is not None:
            self._ws_client.cache_instrument(pyo3_instrument)

        instrument = transform_instrument_from_pyo3(pyo3_instrument)

        self._handle_data(instrument)
