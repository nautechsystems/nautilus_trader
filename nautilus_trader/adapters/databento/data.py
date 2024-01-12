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
from collections import defaultdict
from collections.abc import Coroutine
from functools import partial
from typing import Any

import databento
import pandas as pd
import pytz

from nautilus_trader.adapters.databento.common import databento_schema_from_nautilus_bar_type
from nautilus_trader.adapters.databento.common import nautilus_instrument_id_from_databento
from nautilus_trader.adapters.databento.config import DatabentoDataClientConfig
from nautilus_trader.adapters.databento.constants import ALL_SYMBOLS
from nautilus_trader.adapters.databento.constants import DATABENTO_CLIENT_ID
from nautilus_trader.adapters.databento.constants import ONE_DAY
from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.adapters.databento.parsing import parse_record
from nautilus_trader.adapters.databento.parsing import parse_record_with_metadata
from nautilus_trader.adapters.databento.providers import DatabentoInstrumentProvider
from nautilus_trader.adapters.databento.types import DatabentoPublisher
from nautilus_trader.adapters.databento.types import Dataset
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.nautilus_pyo3 import is_within_last_24_hours
from nautilus_trader.core.nautilus_pyo3 import last_weekday_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import Instrument


class DatabentoDataClient(LiveMarketDataClient):
    """
    Provides a data client for the `Databento` API.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    http_client : databento.Historical
        The Databento historical HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.
    loader : DatabentoDataLoader, optional
        The loader for the client.
    config : DatabentoDataClientConfig, optional
        The configuration for the client.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        http_client: databento.Historical,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        instrument_provider: DatabentoInstrumentProvider,
        loader: DatabentoDataLoader | None = None,
        config: DatabentoDataClientConfig | None = None,
    ) -> None:
        if config is None:
            config = DatabentoDataClientConfig()
        PyCondition.type(config, DatabentoDataClientConfig, "config")

        super().__init__(
            loop=loop,
            client_id=DATABENTO_CLIENT_ID,
            venue=None,  # Not applicable
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            instrument_provider=instrument_provider,
            config=config,
        )

        # Configuration
        self._live_api_key: str = config.api_key or http_client.key
        self._live_gateway: str | None = config.live_gateway
        self._instrument_ids: dict[Dataset, set[InstrumentId]] = defaultdict(set)
        self._timeout_initial_load: float | None = config.timeout_initial_load
        self._mbo_subscriptions_delay: float | None = config.mbo_subscriptions_delay

        # Clients
        self._http_client: databento.Historical = http_client
        self._live_clients: dict[Dataset, databento.Live] = {}
        self._live_clients_mbo: dict[Dataset, databento.Live] = {}
        self._has_subscribed: dict[Dataset, bool] = {}
        self._loader = loader or DatabentoDataLoader()

        # Cache instrument index
        for instrument_id in config.instrument_ids or []:
            dataset: Dataset = self._loader.get_dataset_for_venue(instrument_id.venue)
            self._instrument_ids[dataset].add(instrument_id)

        # MBO/L3 subscription buffering
        self._buffer_mbo_subscriptions_task: asyncio.Task | None = None
        self._is_buffering_mbo_subscriptions: bool = bool(config.mbo_subscriptions_delay)
        self._buffered_mbo_subscriptions: dict[Dataset, list[InstrumentId]] = defaultdict(list)
        self._buffered_deltas: dict[InstrumentId, list[OrderBookDelta]] = defaultdict(list)

    async def _connect(self) -> None:
        if not self._instrument_ids:
            return  # Nothing else to do yet

        if self._is_buffering_mbo_subscriptions:
            self._buffer_mbo_subscriptions_task = self.create_task(self._buffer_mbo_subscriptions())

        self._log.info("Initializing instruments...")

        coros: list[Coroutine] = []
        for dataset, instrument_ids in self._instrument_ids.items():
            loading_ids: list[InstrumentId] = sorted(instrument_ids)
            coros.append(self._instrument_provider.load_ids_async(instrument_ids=loading_ids))
            await self._subscribe_instrument_ids(dataset, instrument_ids=loading_ids)

        try:
            if self._timeout_initial_load:
                await asyncio.wait_for(asyncio.gather(*coros), timeout=self._timeout_initial_load)
            else:
                await asyncio.gather(*coros)
        except asyncio.TimeoutError:
            self._log.warning("Timeout waiting for instruments...")

        self._send_all_instruments_to_data_engine()

    async def _disconnect(self) -> None:
        coros: list[Coroutine] = []
        for dataset, client in self._live_clients.items():
            self._log.info(f"Stopping {dataset} live feed...", LogColor.BLUE)
            coro = client.wait_for_close(timeout=2.0)
            coros.append(coro)

        for dataset, client in self._live_clients_mbo.items():
            self._log.info(f"Stopping {dataset} MBO/L3 live feed...", LogColor.BLUE)
            coro = client.wait_for_close(timeout=2.0)
            coros.append(coro)

        if self._buffer_mbo_subscriptions_task:
            self._log.debug("Canceling `buffer_mbo_subscriptions` task...")
            self._buffer_mbo_subscriptions_task.cancel()
            self._buffer_mbo_subscriptions_task = None

        await asyncio.gather(*coros)

    async def _buffer_mbo_subscriptions(self) -> None:
        try:
            await asyncio.sleep(self._mbo_subscriptions_delay or 0.0)
            self._is_buffering_mbo_subscriptions = False

            coros: list[Coroutine] = []
            for dataset, instrument_ids in self._buffered_mbo_subscriptions.items():
                self._log.info(f"Starting {dataset} MBO/L3 live feeds...")
                coro = self._subscribe_order_book_deltas_batch(instrument_ids)
                coros.append(coro)

            await asyncio.gather(*coros)
        except asyncio.CancelledError:
            self._log.debug("Canceled `buffer_mbo_subscriptions` task.")

    def _get_live_client(self, dataset: Dataset) -> databento.Live:
        # Retrieve or initialize the 'general' live client for the specified dataset
        live_client = self._live_clients.get(dataset)

        if live_client is None:
            live_client = databento.Live(key=self._live_api_key, gateway=self._live_gateway)

            # Wrap the callback with partial to include the dataset
            callback_with_dataset = partial(self._handle_record, live_client.symbology_map)
            live_client.add_callback(callback_with_dataset)
            self._live_clients[dataset] = live_client

        return live_client

    def _get_live_client_mbo(self, dataset: Dataset) -> databento.Live:
        # Retrieve or initialize the 'MBO/L3' live client for the specified dataset
        live_client = self._live_clients_mbo.get(dataset)

        if live_client is None:
            live_client = databento.Live(key=self._live_api_key, gateway=self._live_gateway)

            # Wrap the callback with partial to include the dataset
            callback_with_dataset = partial(self._handle_record, live_client.symbology_map)
            live_client.add_callback(callback_with_dataset)
            self._live_clients_mbo[dataset] = live_client

        return live_client

    def _check_live_client_started(self, dataset: Dataset, live_client: databento.Live) -> None:
        if not self._has_subscribed.get(dataset):
            self._log.debug(f"Starting {dataset} live client...", LogColor.MAGENTA)
            live_client.start()
            self._has_subscribed[dataset] = True
            self._log.info(f"Started {dataset} live feed.", LogColor.BLUE)

    def _send_all_instruments_to_data_engine(self) -> None:
        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)

    async def _ensure_subscribed_for_instrument(self, instrument_id: InstrumentId) -> None:
        dataset: Dataset = self._loader.get_dataset_for_venue(instrument_id.venue)
        subscribed_instruments = self._instrument_ids[dataset]

        if instrument_id in subscribed_instruments:
            return

        self._instrument_ids[dataset].add(instrument_id)
        await self._subscribe_instrument(instrument_id)

    async def _get_dataset_range(self, dataset: Dataset) -> tuple[pd.Timestamp, pd.Timestamp]:
        response = await self._loop.run_in_executor(
            None,
            self._http_client.metadata.get_dataset_range,
            dataset,
        )

        start = pd.Timestamp(response["start_date"], tz=pytz.utc)
        end = pd.Timestamp(response["end_date"], tz=pytz.utc)

        return start, end

    # -- OVERRIDES ----------------------------------------------------------------------------

    def subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int | None = None,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        if book_type != BookType.L3_MBO:
            raise NotImplementedError

        self.create_task(
            self._subscribe_order_book_deltas(
                instrument_id=instrument_id,
                book_type=book_type,
                depth=depth,
                kwargs=kwargs,
            ),
            log_msg=f"subscribe: order_book_deltas {instrument_id}",
            actions=lambda: self._add_subscription_order_book_deltas(instrument_id),
        )

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    async def _subscribe(self, data_type: DataType) -> None:
        # Replace method in child class, for exchange specific data types.
        raise NotImplementedError(f"Cannot subscribe to {data_type.type} (not implemented).")

    async def _subscribe_instruments(self) -> None:
        # Replace method in child class, for exchange specific data types.
        raise NotImplementedError("Cannot subscribe to all instruments (not currently supported).")

    async def _subscribe_instrument(self, instrument_id: InstrumentId) -> None:
        dataset: Dataset = self._loader.get_dataset_for_venue(instrument_id.venue)
        live_client = self._get_live_client(dataset)
        live_client.subscribe(
            dataset=dataset,
            schema=databento.Schema.DEFINITION,
            symbols=[instrument_id.symbol.value],
        )
        self._check_live_client_started(dataset, live_client)

    async def _subscribe_instrument_ids(
        self,
        dataset: Dataset,
        instrument_ids: list[InstrumentId],
    ) -> None:
        live_client = self._get_live_client(dataset)
        live_client.subscribe(
            dataset=dataset,
            schema=databento.Schema.DEFINITION,
            symbols=[i.symbol.value for i in instrument_ids],
        )
        self._check_live_client_started(dataset, live_client)

    async def _subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int | None = None,
        kwargs: dict | None = None,
    ) -> None:
        if book_type != BookType.L3_MBO:
            raise NotImplementedError

        if depth:  # Can be None or 0 (full depth)
            self._log.error(
                f"Cannot subscribe to order book deltas with specific depth of {depth} "
                "(do not specify depth when subscribing, must be full depth).",
            )
            return

        dataset: Dataset = self._loader.get_dataset_for_venue(instrument_id.venue)

        if self._is_buffering_mbo_subscriptions:
            self._log.debug(f"Buffering MBO/L3 subscription for {instrument_id}.", LogColor.MAGENTA)
            self._buffered_mbo_subscriptions[dataset].append(instrument_id)
            return

        if self._get_live_client_mbo(dataset) is not None:
            self._log.error(
                f"Cannot subscribe to order book deltas for {instrument_id}, "
                "MBO/L3 feed already started.",
            )
            return

        await self._subscribe_order_book_deltas_batch([instrument_id])

    async def _subscribe_order_book_deltas_batch(
        self,
        instrument_ids: list[InstrumentId],
    ) -> None:
        if not instrument_ids:
            self._log.warning(
                "No subscriptions for order book deltas (`instrument_ids` was empty).",
            )
            return

        for instrument_id in instrument_ids:
            if not self._cache.instrument(instrument_id):
                self._log.error(
                    f"Cannot subscribe to order book deltas for {instrument_id}, "
                    "instrument must be pre-loaded via the `DatabentoDataClientConfig` "
                    "or a specific subscription on start.",
                )
                instrument_ids.remove(instrument_id)
                continue

        if not instrument_ids:
            return  # No subscribing instrument IDs were loaded in the cache

        dataset: Dataset = self._loader.get_dataset_for_venue(instrument_ids[0].venue)
        live_client = self._get_live_client_mbo(dataset)
        live_client.subscribe(
            dataset=dataset,
            schema=databento.Schema.MBO,
            symbols=[i.symbol.value for i in instrument_ids],
            start=0,  # Must subscribe from start of week to get 'Sunday snapshot' for now
        )
        live_client.start()

    async def _subscribe_order_book_snapshots(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int | None = None,
        kwargs: dict | None = None,
    ) -> None:
        await self._ensure_subscribed_for_instrument(instrument_id)

        match depth:
            case 1:
                schema = databento.Schema.MBP_1
            case 10:
                schema = databento.Schema.MBP_10
            case _:
                self._log.error(
                    f"Cannot subscribe for order book snapshots of depth {depth}, use either 1 or 10.",
                )
                return

        dataset: Dataset = self._loader.get_dataset_for_venue(instrument_id.venue)
        live_client = self._get_live_client(dataset)
        live_client.subscribe(
            dataset=dataset,
            schema=schema,
            symbols=[instrument_id.symbol.value],
        )
        self._check_live_client_started(dataset, live_client)

    async def _subscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        await self._ensure_subscribed_for_instrument(instrument_id)

        dataset: Dataset = self._loader.get_dataset_for_venue(instrument_id.venue)
        live_client = self._get_live_client(dataset)
        live_client.subscribe(
            dataset=dataset,
            schema=databento.Schema.MBP_1,
            symbols=[instrument_id.symbol.value],
        )
        self._check_live_client_started(dataset, live_client)

    async def _subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        if instrument_id in self.subscribed_quote_ticks():
            return  # Already subscribed for trades

        await self._ensure_subscribed_for_instrument(instrument_id)

        dataset: Dataset = self._loader.get_dataset_for_venue(instrument_id.venue)
        live_client = self._get_live_client(dataset)
        live_client.subscribe(
            dataset=dataset,
            schema=databento.Schema.TRADES,
            symbols=[instrument_id.symbol.value],
        )
        self._check_live_client_started(dataset, live_client)

    async def _subscribe_bars(self, bar_type: BarType) -> None:
        dataset: Dataset = self._loader.get_dataset_for_venue(bar_type.instrument_id.venue)

        try:
            schema = databento_schema_from_nautilus_bar_type(bar_type)
        except ValueError as e:
            self._log.error(f"Cannot subscribe: {e}")
            return

        live_client = self._get_live_client(dataset)
        live_client.subscribe(
            dataset=dataset,
            schema=schema,
            symbols=[bar_type.instrument_id.symbol.value],
        )
        self._check_live_client_started(dataset, live_client)

    async def _unsubscribe(self, data_type: DataType) -> None:
        raise NotImplementedError(
            f"Cannot unsubscribe from {data_type} (not implemented).",
        )

    async def _unsubscribe_instruments(self) -> None:
        raise NotImplementedError(
            "Cannot unsubscribe from all instruments, unsubscribing not supported by Databento.",
        )

    async def _unsubscribe_instrument(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(
            f"Cannot unsubscribe from {instrument_id} instrument, "
            "unsubscribing not supported by Databento.",
        )

    async def _unsubscribe_order_book_deltas(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(
            f"Cannot unsubscribe from {instrument_id} order book deltas, "
            "unsubscribing not supported by Databento.",
        )

    async def _unsubscribe_order_book_snapshots(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(
            f"Cannot unsubscribe from {instrument_id} order book snapshots, "
            "unsubscribing not supported by Databento.",
        )

    async def _unsubscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(
            f"Cannot unsubscribe from {instrument_id} quote ticks, "
            "unsubscribing not supported by Databento.",
        )

    async def _unsubscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(
            f"Cannot unsubscribe from {instrument_id} trade ticks, "
            "unsubscribing not supported by Databento.",
        )

    async def _unsubscribe_bars(self, bar_type: BarType) -> None:
        raise NotImplementedError(
            f"Cannot unsubscribe from {bar_type} bars, "
            "unsubscribing not supported by Databento.",
        )

    async def _request(self, data_type: DataType, correlation_id: UUID4) -> None:
        raise NotImplementedError(
            f"Cannot request {data_type} (not implemented).",
        )

    async def _request_instrument(
        self,
        instrument_id: InstrumentId,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        dataset: Dataset = self._loader.get_dataset_for_venue(instrument_id.venue)

        _, available_end = await self._get_dataset_range(dataset)

        date_now_utc = self._clock.utc_now().date()
        default_start = pd.Timestamp(
            last_weekday_nanos(
                year=date_now_utc.year,
                month=date_now_utc.month,
                day=date_now_utc.day,
            ),
            tz=pytz.utc,
        )

        default_start = min(default_start, available_end - ONE_DAY)

        data = await self._http_client.timeseries.get_range_async(
            dataset=dataset,
            start=start or default_start.date().isoformat(),
            end=end,
            symbols=instrument_id.symbol.value,
            schema=databento.Schema.DEFINITION,
        )

        for record in data:
            instrument = parse_record(
                record=record,
                instrument_id=instrument_id,
                ts_init=self._clock.timestamp_ns(),
            )

            self._handle_instrument(
                instrument=instrument,
                correlation_id=correlation_id,
            )

    async def _request_instruments(
        self,
        venue: Venue,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        dataset: Dataset = self._loader.get_dataset_for_venue(venue)

        _, available_end = await self._get_dataset_range(dataset)

        date_now_utc = self._clock.utc_now().date()
        default_start = pd.Timestamp(
            last_weekday_nanos(
                year=date_now_utc.year,
                month=date_now_utc.month,
                day=date_now_utc.day,
            ),
            tz=pytz.utc,
        )

        default_start = min(default_start, available_end - ONE_DAY * 2)

        data = await self._http_client.timeseries.get_range_async(
            dataset=dataset,
            start=start or default_start.date().isoformat(),
            end=end,
            symbols=ALL_SYMBOLS,
            schema=databento.Schema.DEFINITION,
        )

        instruments: list[Instrument] = []

        for record in data:
            try:
                instrument = parse_record_with_metadata(
                    record=record,
                    publishers=self._loader.publishers,
                    ts_init=self._clock.timestamp_ns(),
                )
            except ValueError as e:
                self._log.error(repr(e))
                continue

            instruments.append(instrument)

        self._handle_instruments(
            instruments=instruments,
            venue=venue,
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
        dataset: Dataset = self._loader.get_dataset_for_venue(instrument_id.venue)

        date_now_utc = self._clock.utc_now().date()
        default_start = pd.Timestamp(
            last_weekday_nanos(
                year=date_now_utc.year,
                month=date_now_utc.month,
                day=date_now_utc.day,
            ),
            tz=pytz.utc,
        )

        if is_within_last_24_hours(default_start.value):
            default_start -= ONE_DAY

        data = await self._http_client.timeseries.get_range_async(
            dataset=dataset,
            start=start or default_start.date().isoformat(),
            end=end,
            symbols=instrument_id.symbol.value,
            schema=databento.Schema.MBP_1,
            limit=limit,
        )

        ticks: list[QuoteTick] = []

        for record in data:
            tick = parse_record(
                record=record,
                instrument_id=instrument_id,
                ts_init=self._clock.timestamp_ns(),
            )

            if not isinstance(tick, QuoteTick):
                # Might be `TradeTick`
                continue

            ticks.append(tick)

        self._handle_quote_ticks(
            instrument_id=instrument_id,
            ticks=ticks,
            correlation_id=correlation_id,
        )

    async def _request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        dataset: Dataset = self._loader.get_dataset_for_venue(instrument_id.venue)

        date_now_utc = self._clock.utc_now().date()
        default_start = pd.Timestamp(
            last_weekday_nanos(
                year=date_now_utc.year,
                month=date_now_utc.month,
                day=date_now_utc.day,
            ),
            tz=pytz.utc,
        )

        if is_within_last_24_hours(default_start.value):
            default_start -= ONE_DAY

        data = await self._http_client.timeseries.get_range_async(
            dataset=dataset,
            start=start or default_start.date().isoformat(),
            end=end,
            symbols=instrument_id.symbol.value,
            schema=databento.Schema.TRADES,
            limit=limit,
        )

        ticks: list[TradeTick] = []

        for record in data:
            tick = parse_record(
                record=record,
                instrument_id=instrument_id,
                ts_init=self._clock.timestamp_ns(),
            )

            ticks.append(tick)

        self._handle_trade_ticks(
            instrument_id=instrument_id,
            ticks=ticks,
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
        try:
            schema = databento_schema_from_nautilus_bar_type(bar_type)
        except ValueError as e:
            self._log.error(f"Cannot request: {e}")
            return

        dataset: Dataset = self._loader.get_dataset_for_venue(bar_type.instrument_id.venue)

        date_now_utc = self._clock.utc_now().date()
        default_start = pd.Timestamp(
            last_weekday_nanos(
                year=date_now_utc.year,
                month=date_now_utc.month,
                day=date_now_utc.day,
            ),
            tz=pytz.utc,
        )

        if is_within_last_24_hours(default_start.value):
            default_start -= ONE_DAY

        data = await self._http_client.timeseries.get_range_async(
            dataset=dataset,
            start=start or default_start.date().isoformat(),
            end=end,
            symbols=bar_type.instrument_id.symbol.value,
            schema=schema,
            limit=limit,
        )

        bars: list[Bar] = []

        for record in data:
            bar = parse_record(
                record=record,
                instrument_id=bar_type.instrument_id,
                ts_init=self._clock.timestamp_ns(),
            )

            bars.append(bar)

        self._handle_bars(
            bar_type=bar_type,
            bars=bars,
            partial=None,  # No partials
            correlation_id=correlation_id,
        )

    def _handle_record(
        self,
        instrument_map: dict[int, str | int],
        record: databento.DBNRecord,
    ) -> None:
        # self._log.debug(f"Received {record}", LogColor.MAGENTA)
        if isinstance(record, databento.ErrorMsg):
            self._log.error(f"ErrorMsg: {record.err}")
            return
        elif isinstance(record, databento.SystemMsg):
            self._log.info(f"SystemMsg: {record.msg}")
            return
        elif isinstance(record, databento.SymbolMappingMsg):
            self._log.debug(f"SymbolMappingMsg: {record}")
            return

        try:
            raw_symbol = instrument_map.get(record.instrument_id)
            if raw_symbol is None:
                raise ValueError(f"Cannot resolve instrument_id {record.instrument_id}")

            publisher: DatabentoPublisher = self._loader.publishers[record.publisher_id]
            instrument_id: InstrumentId = nautilus_instrument_id_from_databento(
                raw_symbol=str(raw_symbol),
                publisher=publisher,
            )
            data = parse_record(record, instrument_id, ts_init=self._clock.timestamp_ns())
        except ValueError as e:
            self._log.error(f"{e!r}")
            return

        if isinstance(data, OrderBookDelta):
            if databento.RecordFlags.F_LAST not in databento.RecordFlags(data.flags):
                buffer = self._buffered_deltas[data.instrument_id]
                buffer.append(data)
                return  # We can rely on the F_LAST flag for an MBO feed
            else:
                buffer = self._buffered_deltas[data.instrument_id]
                buffer.append(data)
                data = OrderBookDeltas(instrument_id, deltas=buffer.copy())
                buffer.clear()

        if isinstance(data, tuple):
            self._handle_data(data[0])
            self._handle_data(data[1])
        else:
            self._handle_data(data)
