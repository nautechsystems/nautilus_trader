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

from nautilus_trader.adapters.bitmex.config import BitmexDataClientConfig
from nautilus_trader.adapters.bitmex.constants import BITMEX_VENUE
from nautilus_trader.adapters.bitmex.providers import BitmexInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
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
from nautilus_trader.live.cancellation import DEFAULT_FUTURE_CANCELLATION_TIMEOUT
from nautilus_trader.live.cancellation import cancel_tasks_with_timeout
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import FundingRateUpdate
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import book_type_to_str
from nautilus_trader.model.identifiers import ClientId


class BitmexDataClient(LiveMarketDataClient):
    """
    Provides a data client for the BitMEX centralized crypto exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.BitmexHttpClient
        The BitMEX HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : BitmexInstrumentProvider
        The instrument provider.
    config : BitMEXDataClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.BitmexHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: BitmexInstrumentProvider,
        config: BitmexDataClientConfig,
        name: str | None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or BITMEX_VENUE.value),
            venue=BITMEX_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        # Configuration
        self._config = config
        self._symbol_status = config.symbol_status
        self._log.info(f"config.symbol_status={config.symbol_status}", LogColor.BLUE)
        self._log.info(f"config.testnet={config.testnet}", LogColor.BLUE)
        self._log.info(f"config.http_timeout_secs={config.http_timeout_secs}", LogColor.BLUE)
        self._log.info(
            f"config.update_instruments_interval_mins={config.update_instruments_interval_mins}",
            LogColor.BLUE,
        )

        # Periodic updates
        self._update_instruments_interval_mins: int | None = config.update_instruments_interval_mins
        self._update_instruments_task: asyncio.Task | None = None

        # HTTP API
        self._http_client = client
        self._log.info(f"REST API key {self._http_client.api_key}", LogColor.BLUE)

        # WebSocket API
        ws_url = self._determine_ws_url(config)
        self._ws_client = nautilus_pyo3.BitmexWebSocketClient(
            url=ws_url,
            api_key=config.api_key,
            api_secret=config.api_secret,
            heartbeat=30,
        )
        self._ws_client_futures: set[asyncio.Future] = set()
        self._log.info(f"WebSocket URL {ws_url}", LogColor.BLUE)

    @property
    def instrument_provider(self) -> BitmexInstrumentProvider:
        return self._instrument_provider  # type: ignore

    async def _connect(self) -> None:
        # Initialize instruments
        await self._instrument_provider.initialize()
        self._send_all_instruments_to_data_engine()

        # Connect WebSocket client (non-blocking)
        future = asyncio.ensure_future(
            self._ws_client.connect(
                self._handle_msg,
            ),
        )
        self._ws_client_futures.add(future)
        self._log.info(f"Connected to WebSocket {self._ws_client.url}", LogColor.BLUE)

        # Start periodic instrument updates if configured
        if self._update_instruments_interval_mins:
            self._update_instruments_task = self.create_task(
                self._update_instruments(self._update_instruments_interval_mins),
            )

    async def _disconnect(self) -> None:
        # Cancel periodic update task if running
        if self._update_instruments_task:
            self._log.debug("Canceling update instruments task")
            self._update_instruments_task.cancel()
            try:
                await asyncio.wait_for(self._update_instruments_task, timeout=2.0)
            except (TimeoutError, asyncio.CancelledError):
                pass
            self._update_instruments_task = None

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

    def _determine_ws_url(self, config: BitmexDataClientConfig) -> str:
        """
        Determine the WebSocket URL based on configuration.
        """
        if config.base_url_ws:
            return config.base_url_ws
        elif config.testnet:
            return "wss://testnet.bitmex.com/realtime"
        else:
            return "wss://ws.bitmex.com/realtime"

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        if command.book_type != BookType.L2_MBP:
            self._log.warning(
                f"Book type {book_type_to_str(command.book_type)} not supported by BitMEX, skipping subscription",
            )
            return

        if command.depth not in (0, 25):
            self._log.error(
                "Cannot subscribe to order book deltas: "
                f"invalid `depth`, was {command.depth}; "
                "valid depths are 0 (default full book), or 25",
            )
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)

        if command.depth == 25:
            await self._ws_client.subscribe_book_25(pyo3_instrument_id)
        else:
            await self._ws_client.subscribe_book(pyo3_instrument_id)

    async def _subscribe_order_book_snapshots(self, command: SubscribeOrderBook) -> None:
        if command.book_type != BookType.L2_MBP:
            self._log.warning(
                f"Book type {book_type_to_str(command.book_type)} not supported by BitMEX, skipping subscription",
            )
            return

        if command.depth not in (0, 10):
            self._log.error(
                "Cannot subscribe to order book snapshots: "
                f"invalid `depth`, was {command.depth}; "
                "valid depths are 0 (default 10), or 10",
            )
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)

        await self._ws_client.subscribe_book_depth10(pyo3_instrument_id)

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_quotes(pyo3_instrument_id)

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_trades(pyo3_instrument_id)

    async def _subscribe_instruments(self, command: SubscribeInstruments) -> None:
        # Subscribe to instrument updates for the entire venue via WebSocket
        await self._ws_client.subscribe_instruments()

    async def _subscribe_instrument(self, command: SubscribeInstrument) -> None:
        # Subscribe to instrument updates for specific instrument via WebSocket
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_instrument(pyo3_instrument_id)

    async def _subscribe_mark_prices(self, command) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_mark_prices(pyo3_instrument_id)

    async def _subscribe_index_prices(self, command) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_index_prices(pyo3_instrument_id)

    async def _subscribe_funding_rates(self, command) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_funding_rates(pyo3_instrument_id)

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(command.bar_type))
        await self._ws_client.subscribe_bars(pyo3_bar_type)

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_book(pyo3_instrument_id)
        await self._ws_client.unsubscribe_book_25(pyo3_instrument_id)

    async def _unsubscribe_order_book_snapshots(self, command: UnsubscribeOrderBook) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_book_depth10(pyo3_instrument_id)

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_quotes(pyo3_instrument_id)

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_trades(pyo3_instrument_id)

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(command.bar_type))
        await self._ws_client.unsubscribe_bars(pyo3_bar_type)

    async def _unsubscribe_instruments(self, command: UnsubscribeInstruments) -> None:
        # Unsubscribe from all instrument updates for the venue
        await self._ws_client.unsubscribe_instruments()

    async def _unsubscribe_instrument(self, command: UnsubscribeInstrument) -> None:
        # Unsubscribe from specific instrument updates
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_instrument(pyo3_instrument_id)

    async def _unsubscribe_mark_prices(self, command) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_mark_prices(pyo3_instrument_id)

    async def _unsubscribe_index_prices(self, command) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_index_prices(pyo3_instrument_id)

    async def _unsubscribe_funding_rates(self, command) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_funding_rates(pyo3_instrument_id)

    async def _request_instruments(self, request: RequestInstruments) -> None:
        instruments = await self._http_client.request_instruments(self._symbol_status)
        for instrument in instruments:
            self._handle_instrument(instrument)
        self._send_response(
            msg_type=type(request),
            correlation_id=request.id,
        )

    async def _request_instrument(self, request: RequestInstrument) -> None:
        instruments = await self._http_client.request_instruments(self._symbol_status)
        for instrument in instruments:
            if instrument.id == request.instrument_id:
                self._handle_instrument(instrument)
                self._send_response(
                    msg_type=type(request),
                    correlation_id=request.id,
                )
                return

        self._log.warning(f"Instrument {request.instrument_id} not found")

    async def _request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        # TODO: Implement
        self._log.warning("Quote ticks request not yet implemented")

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        # TODO: Implement
        self._log.warning("Trade ticks request not yet implemented")

    async def _request_bars(self, request: RequestBars) -> None:
        # TODO: Implement
        self._log.warning("Bars request not yet implemented")

    async def _update_instruments(self, interval_mins: int) -> None:
        """
        Periodically update instruments from the venue.
        """
        while True:
            try:
                self._log.debug(
                    f"Scheduled task 'update_instruments' to run in {interval_mins} minutes",
                )
                await asyncio.sleep(interval_mins * 60)
                await self._instrument_provider.initialize(reload=True)
                self._send_all_instruments_to_data_engine()
            except asyncio.CancelledError:
                self._log.debug("Canceled task 'update_instruments'")
                return
            except Exception as e:
                self._log.error(f"Error updating instruments: {e}")

    def _send_all_instruments_to_data_engine(self) -> None:
        """
        Send all instruments to the data engine.
        """
        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)

        for currency in self._instrument_provider.currencies().values():
            self._cache.add_currency(currency)

    def _handle_msg(self, msg: Any) -> None:
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
                self._log.warning(f"Cannot handle message {msg}, not implemented")
        except Exception as e:
            self._log.exception("Error handling websocket message", e)
