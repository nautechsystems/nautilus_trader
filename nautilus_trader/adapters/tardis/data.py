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

import pandas as pd

from nautilus_trader.adapters.tardis.common import convert_nautilus_bar_type_to_tardis_data_type
from nautilus_trader.adapters.tardis.common import convert_nautilus_data_type_to_tardis_data_type
from nautilus_trader.adapters.tardis.common import create_instrument_info
from nautilus_trader.adapters.tardis.common import create_stream_normalized_request_options
from nautilus_trader.adapters.tardis.common import get_ws_client_key
from nautilus_trader.adapters.tardis.config import TardisDataClientConfig
from nautilus_trader.adapters.tardis.constants import TARDIS
from nautilus_trader.adapters.tardis.providers import TardisInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import Instrument


class TardisDataClient(LiveMarketDataClient):
    """
    Provides a data client for the Tardis data provider.

    Both instrument metadata HTTP API and Tardis Machine API are leveraged
    to provide historical data for requests, and live data feeds based on subscriptions.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : TardisInstrumentProvider
        The instrument provider.
    config : TardisDataClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: TardisInstrumentProvider,
        config: TardisDataClientConfig,
        name: str | None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or TARDIS),
            venue=None,  # Not applicable
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        # Configuration
        self._config = config
        self._log.info(f"{config.update_instruments_interval_mins=}", LogColor.BLUE)

        # Tardis Machine
        self._ws_base_url = self._config.base_url_ws
        self._ws_client: nautilus_pyo3.TardisMachineClient = self._create_websocket_client()
        self._ws_clients: dict[str, nautilus_pyo3.TardisMachineClient] = {}
        self._ws_pending_infos: list[nautilus_pyo3.InstrumentMiniInfo] = []
        self._ws_pending_streams: list[nautilus_pyo3.StreamNormalizedRequestOptions] = []
        self._ws_client_futures: set[asyncio.Future] = set()

        # Tasks
        self._update_instruments_interval_mins: int | None = config.update_instruments_interval_mins
        self._update_instruments_task: asyncio.Task | None = None
        self._main_ws_connect_task: asyncio.Task | None = None
        self._main_ws_delay = True

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        self._send_all_instruments_to_data_engine()

        if self._update_instruments_interval_mins:
            self._update_instruments_task = self.create_task(
                self._update_instruments(self._update_instruments_interval_mins),
            )

        self._main_ws_connect_task = self.create_task(self._connect_main_ws_after_delay())

    async def _disconnect(self) -> None:
        if self._update_instruments_task:
            self._log.debug("Canceling task 'update_instruments'")
            self._update_instruments_task.cancel()
            self._update_instruments_task = None

        if self._main_ws_connect_task:
            self._log.debug("Canceling task 'connect_main_ws_after_delay'")
            self._main_ws_connect_task.cancel()
            self._main_ws_connect_task = None

        # Shutdown websockets
        if not self._ws_client.is_closed():
            self._ws_client.close()

        for ws_client in self._ws_clients.values():
            if not ws_client.is_closed():
                ws_client.close()
        self._ws_clients.clear()

        try:
            await asyncio.wait_for(asyncio.gather(*self._ws_client_futures), timeout=2.0)
        except TimeoutError:
            self._log.warning("Timeout while waiting for websockets shutdown to complete")

        self._ws_client_futures.clear()

        self._main_ws_delay = True

    def _create_websocket_client(self) -> nautilus_pyo3.TardisMachineClient:
        self._log.info("Creating new TardisMachineClient", LogColor.MAGENTA)
        return nautilus_pyo3.TardisMachineClient(
            base_url=self._ws_base_url,
            normalize_symbols=True,
        )

    def _dispose_websocket_client_by_key(self, ws_client_key: str) -> None:
        ws_client = self._ws_clients.pop(ws_client_key, None)
        if ws_client:
            ws_client.close()

    async def _connect_main_ws_after_delay(self) -> None:
        delay_secs = self._config.ws_connection_delay_secs
        self._log.info(
            f"Awaiting initial websocket connection delay ({delay_secs}s)...",
            LogColor.BLUE,
        )
        await asyncio.sleep(delay_secs)
        if self._ws_pending_streams:
            future = asyncio.ensure_future(
                self._ws_client.stream(
                    instruments=self._ws_pending_infos,
                    options=self._ws_pending_streams,
                    callback=self._handle_msg,
                ),
            )
            self._ws_client_futures.add(future)
            self._ws_pending_infos.clear()
            self._ws_pending_streams.clear()

        self._main_ws_delay = False

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

    def _subscribe_stream(
        self,
        instrument_id: InstrumentId,
        tardis_data_type: str,
        data_type: str,
    ) -> None:
        instrument = self._cache.instrument(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot subscribe {data_type}: no instrument for {instrument_id}")
            return

        instrument_info = create_instrument_info(instrument)
        tardis_exchange_str = instrument_info.exchange
        raw_symbol_str = instrument.raw_symbol.value
        self._log.info(
            f"Subscribing stream: exchange={tardis_exchange_str}, raw_symbol={raw_symbol_str}, data_type={tardis_data_type}",
            LogColor.MAGENTA,
        )
        stream_request = create_stream_normalized_request_options(
            exchange=tardis_exchange_str,
            symbols=[raw_symbol_str],
            data_types=[tardis_data_type],
            timeout_interval_ms=5_000,
        )

        if self._main_ws_delay:
            self._ws_pending_infos.append(instrument_info)
            self._ws_pending_streams.append(stream_request)
            return

        ws_client = self._create_websocket_client()
        future = asyncio.ensure_future(
            ws_client.stream(
                instruments=[instrument_info],
                options=[stream_request],
                callback=self._handle_msg,
            ),
        )
        self._ws_client_futures.add(future)

        ws_client_key = get_ws_client_key(instrument_id, tardis_data_type)
        self._ws_clients[ws_client_key] = ws_client

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
                "L3_MBO data is not published by Tardis. "
                "Valid book types are L1_MBP, L2_MBP",
            )
            return

        tardis_data_type = convert_nautilus_data_type_to_tardis_data_type(OrderBookDelta)
        self._subscribe_stream(instrument_id, tardis_data_type, "order book deltas")

    async def _subscribe_order_book_snapshots(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int | None = None,
        kwargs: dict | None = None,
    ) -> None:
        if book_type == BookType.L3_MBO:
            self._log.error(
                "Cannot subscribe to order book snapshots: "
                "L3_MBO data is not published by Tardis. "
                "Valid book types are L1_MBP, L2_MBP",
            )
            return

        tardis_data_type = convert_nautilus_data_type_to_tardis_data_type(OrderBookDepth10)
        tardis_data_type = f"{tardis_data_type}_{depth}_0ms"
        self._subscribe_stream(instrument_id, tardis_data_type, "order book snapshots")

    async def _subscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        tardis_data_type = convert_nautilus_data_type_to_tardis_data_type(QuoteTick)
        self._subscribe_stream(instrument_id, tardis_data_type, "quotes")

    async def _subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        tardis_data_type = convert_nautilus_data_type_to_tardis_data_type(TradeTick)
        self._subscribe_stream(instrument_id, tardis_data_type, "trades")

    async def _subscribe_bars(self, bar_type: BarType) -> None:
        tardis_data_type = convert_nautilus_bar_type_to_tardis_data_type(bar_type)
        self._subscribe_stream(bar_type.instrument_id, tardis_data_type, "bars")

    async def _unsubscribe_order_book_deltas(self, instrument_id: InstrumentId) -> None:
        tardis_data_type = convert_nautilus_data_type_to_tardis_data_type(OrderBookDelta)
        ws_client_key = get_ws_client_key(instrument_id, tardis_data_type)
        self._dispose_websocket_client_by_key(ws_client_key)

    async def _unsubscribe_order_book_snapshots(self, instrument_id: InstrumentId) -> None:
        tardis_data_type = convert_nautilus_data_type_to_tardis_data_type(OrderBookDepth10)
        ws_client_key = get_ws_client_key(instrument_id, tardis_data_type)
        self._dispose_websocket_client_by_key(ws_client_key)

    async def _unsubscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        tardis_data_type = convert_nautilus_data_type_to_tardis_data_type(QuoteTick)
        ws_client_key = get_ws_client_key(instrument_id, tardis_data_type)
        self._dispose_websocket_client_by_key(ws_client_key)

    async def _unsubscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        tardis_data_type = convert_nautilus_data_type_to_tardis_data_type(TradeTick)
        ws_client_key = get_ws_client_key(instrument_id, tardis_data_type)
        self._dispose_websocket_client_by_key(ws_client_key)

    async def _unsubscribe_bars(self, bar_type: BarType) -> None:
        tardis_data_type = convert_nautilus_bar_type_to_tardis_data_type(bar_type)
        ws_client_key = get_ws_client_key(bar_type.instrument_id, tardis_data_type)
        self._dispose_websocket_client_by_key(ws_client_key)

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
            f"Cannot request historical quotes for {instrument_id}: not supported in this version",
        )

    async def _request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        self._log.error(
            f"Cannot request historical trades for {instrument_id}: not supported in this version",
        )

    async def _request_bars(
        self,
        bar_type: BarType,
        limit: int,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        self._log.error(
            f"Cannot request historical bars for {bar_type}: not supported in this version",
        )

    def _handle_msg(
        self,
        pycapsule: object,
    ) -> None:
        # The capsule will fall out of scope at the end of this method,
        # and eventually be garbage collected. The contained pointer
        # to `Data` is still owned and managed by Rust.
        data = capsule_to_data(pycapsule)
        self._handle_data(data)
