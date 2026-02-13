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
from collections import defaultdict
from collections.abc import Coroutine

import pandas as pd

from nautilus_trader.adapters.databento.common import databento_schema_from_nautilus_bar_type
from nautilus_trader.adapters.databento.common import instrument_id_to_pyo3
from nautilus_trader.adapters.databento.config import DatabentoDataClientConfig
from nautilus_trader.adapters.databento.constants import ALL_SYMBOLS
from nautilus_trader.adapters.databento.constants import DATABENTO
from nautilus_trader.adapters.databento.constants import PUBLISHERS_FILEPATH
from nautilus_trader.adapters.databento.enums import DatabentoSchema
from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.adapters.databento.providers import DatabentoInstrumentProvider
from nautilus_trader.adapters.databento.types import DatabentoImbalance
from nautilus_trader.adapters.databento.types import DatabentoStatistics
from nautilus_trader.adapters.databento.types import DatabentoSubscriptionAck
from nautilus_trader.adapters.databento.types import Dataset
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestData
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestOrderBookDeltas
from nautilus_trader.data.messages import RequestOrderBookDepth
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeData
from nautilus_trader.data.messages import SubscribeInstrument
from nautilus_trader.data.messages import SubscribeInstruments
from nautilus_trader.data.messages import SubscribeInstrumentStatus
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeData
from nautilus_trader.data.messages import UnsubscribeInstrument
from nautilus_trader.data.messages import UnsubscribeInstruments
from nautilus_trader.data.messages import UnsubscribeInstrumentStatus
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.cancellation import DEFAULT_FUTURE_CANCELLATION_TIMEOUT
from nautilus_trader.live.cancellation import cancel_tasks_with_timeout
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import InstrumentStatus
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.model.enums import bar_aggregation_to_str
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import instruments_from_pyo3


class DatabentoDataClient(LiveMarketDataClient):
    """
    Provides a data client for the Databento API.

    Both Historical and Live APIs are leveraged to provide historical data
    for requests, and live data feeds based on subscriptions.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    http_client : nautilus_pyo3.DatabentoHistoricalClient
        The Databento historical HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : DatabentoInstrumentProvider
        The instrument provider for the client.
    loader : DatabentoDataLoader, optional
        The loader for the client.
    config : DatabentoDataClientConfig, optional
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        http_client: nautilus_pyo3.DatabentoHistoricalClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: DatabentoInstrumentProvider,
        loader: DatabentoDataLoader | None = None,
        config: DatabentoDataClientConfig | None = None,
        name: str | None = None,
    ) -> None:
        if config is None:
            config = DatabentoDataClientConfig()

        PyCondition.type(config, DatabentoDataClientConfig, "config")

        super().__init__(
            loop=loop,
            client_id=ClientId(name or DATABENTO),
            venue=None,  # Not applicable
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
            config=config,
        )

        # Configuration
        self._live_api_key: str = config.api_key or http_client.key
        self._live_gateway: str | None = config.live_gateway
        self._use_exchange_as_venue: bool = config.use_exchange_as_venue
        self._timeout_initial_load: float | None = config.timeout_initial_load
        self._mbo_subscriptions_delay: float | None = config.mbo_subscriptions_delay
        self._bars_timestamp_on_close: bool = config.bars_timestamp_on_close
        self._reconnect_timeout_mins: int | None = config.reconnect_timeout_mins
        self._parent_symbols: dict[Dataset, set[str]] = defaultdict(set)
        self._venue_dataset_map: dict[Venue, Dataset] | None = config.venue_dataset_map
        self._instrument_ids: dict[Dataset, set[InstrumentId]] = defaultdict(set)

        self._log.info(f"{config.use_exchange_as_venue=}", LogColor.BLUE)
        self._log.info(f"{config.timeout_initial_load=}", LogColor.BLUE)
        self._log.info(f"{config.mbo_subscriptions_delay=}", LogColor.BLUE)
        self._log.info(f"{config.bars_timestamp_on_close=}", LogColor.BLUE)
        self._log.info(f"{config.reconnect_timeout_mins=}", LogColor.BLUE)

        # Clients
        self._http_client = http_client
        self._live_clients: dict[Dataset, nautilus_pyo3.DatabentoLiveClient] = {}
        self._live_clients_mbo: dict[Dataset, nautilus_pyo3.DatabentoLiveClient] = {}
        self._has_subscribed: dict[Dataset, bool] = {}
        self._loader = loader or DatabentoDataLoader()
        self._dataset_ranges: dict[Dataset, tuple[pd.Timestamp, pd.Timestamp]] = {}
        self._dataset_ranges_requested: set[Dataset] = set()
        self._trade_tick_subscriptions: set[InstrumentId] = set()

        # Cache parent symbol index
        for dataset, parent_symbols in (config.parent_symbols or {}).items():
            self._parent_symbols[dataset].update(set(parent_symbols))

        # Cache instrument index
        for instrument_id in config.instrument_ids or []:
            dataset = self._loader.get_dataset_for_venue(instrument_id.venue)
            self._instrument_ids[dataset].add(instrument_id)

        # MBO/L3 subscription buffering
        self._buffer_mbo_subscriptions_task: asyncio.Task | None = None
        self._is_buffering_mbo_subscriptions: bool = bool(config.mbo_subscriptions_delay)
        self._buffered_mbo_subscriptions: dict[Dataset, list[InstrumentId]] = defaultdict(list)

        # Tasks
        self._live_client_futures: set[asyncio.Future] = set()
        self._update_dataset_ranges_interval_secs: int = 60 * 60  # Once per hour (hardcoded)
        self._update_dataset_ranges_task: asyncio.Task | None = None

    async def _connect(self) -> None:
        if not self._instrument_ids:
            return  # Nothing else to do yet

        if self._is_buffering_mbo_subscriptions:
            self._buffer_mbo_subscriptions_task = self.create_task(self._buffer_mbo_subscriptions())

        self._log.info("Initializing instruments...")

        coros: list[Coroutine] = []

        for dataset, instrument_ids in self._instrument_ids.items():
            loading_ids: list[InstrumentId] = sorted(instrument_ids)
            filters = {
                "use_exchange_as_venue": self._use_exchange_as_venue,
                "parent_symbols": list(self._parent_symbols.get(dataset, [])),
            }
            coro = self._instrument_provider.load_ids_async(
                instrument_ids=loading_ids,
                filters=filters,
            )
            coros.append(coro)
            await self._subscribe_instrument_ids(dataset, instrument_ids=loading_ids)

        try:
            if self._timeout_initial_load:
                await asyncio.wait_for(asyncio.gather(*coros), timeout=self._timeout_initial_load)
            else:
                await asyncio.gather(*coros)
        except TimeoutError:
            self._log.warning("Timeout waiting for instruments")

        self._send_all_instruments_to_data_engine()
        self._update_dataset_ranges_task = self.create_task(self._update_dataset_ranges())

    async def _disconnect(self) -> None:
        if self._buffer_mbo_subscriptions_task:
            self._log.debug("Canceling task 'buffer_mbo_subscriptions'")
            self._buffer_mbo_subscriptions_task.cancel()
            self._buffer_mbo_subscriptions_task = None

        if self._update_dataset_ranges_task:
            self._log.debug("Canceling task 'update_dataset_ranges'")
            self._update_dataset_ranges_task.cancel()
            self._update_dataset_ranges_task = None

        await self._close_live_clients()
        await self._cancel_pending_futures()

    async def _close_live_clients(self) -> None:
        for dataset, live_client in self._live_clients.items():
            if not live_client.is_running():
                continue

            self._log.info(f"Stopping {dataset} live feed", LogColor.BLUE)
            live_client.close()

        for dataset, live_client in self._live_clients_mbo.items():
            if not live_client.is_running():
                continue

            self._log.info(f"Stopping {dataset} MBO/L3 live feed", LogColor.BLUE)
            live_client.close()

    async def _cancel_pending_futures(self) -> None:
        await cancel_tasks_with_timeout(
            self._live_client_futures,
            self._log,
            timeout_secs=DEFAULT_FUTURE_CANCELLATION_TIMEOUT,
        )
        self._live_client_futures.clear()

    async def _update_dataset_ranges(self) -> None:
        while True:
            try:
                self._log.debug(
                    f"Scheduled task 'update_instruments' to run in "
                    f"{self._update_dataset_ranges_interval_secs}s",
                )

                await asyncio.sleep(self._update_dataset_ranges_interval_secs)
                tasks = []

                for dataset in self._dataset_ranges:
                    tasks.append(self._get_dataset_range(dataset))

                await asyncio.gather(*tasks)
            except Exception as e:  # Create specific exception type
                self._log.exception("Error updating dataset range", e)
            except asyncio.CancelledError:
                self._log.debug("Canceled task 'update_dataset_ranges'")
                break

    async def _buffer_mbo_subscriptions(self) -> None:
        try:
            self._log.debug("Buffering MBO subscriptions...", LogColor.MAGENTA)
            await asyncio.sleep(self._mbo_subscriptions_delay or 0.0)
            self._is_buffering_mbo_subscriptions = False
            coros: list[Coroutine] = []

            for dataset, instrument_ids in self._buffered_mbo_subscriptions.items():
                self._log.info(f"Starting {dataset} MBO/L3 live feeds")
                coro = self._subscribe_order_book_deltas_batch(instrument_ids)
                coros.append(coro)

            await asyncio.gather(*coros)
        except asyncio.CancelledError:
            self._log.debug("Canceled task 'buffer_mbo_subscriptions'")

    def _log_future_exception_callback(self, future: asyncio.Future) -> None:
        if future.cancelled():
            return  # Normal cancellation

        exc = future.exception()
        if exc:
            self._log.error(f"Future raised: {exc}")

    def _get_live_client(self, dataset: Dataset) -> nautilus_pyo3.DatabentoLiveClient:
        # Retrieve or initialize the 'general' live client for the specified dataset
        live_client = self._live_clients.get(dataset)

        if live_client is None:
            live_client = nautilus_pyo3.DatabentoLiveClient(
                key=self._live_api_key,
                dataset=dataset,
                publishers_filepath=str(PUBLISHERS_FILEPATH),
                use_exchange_as_venue=self._use_exchange_as_venue,
                bars_timestamp_on_close=self._bars_timestamp_on_close,
                reconnect_timeout_mins=self._reconnect_timeout_mins,
            )
            self._live_clients[dataset] = live_client

        return live_client

    def _get_live_client_mbo(self, dataset: Dataset) -> nautilus_pyo3.DatabentoLiveClient:
        # Retrieve or initialize the 'MBO/L3' live client for the specified dataset
        live_client = self._live_clients_mbo.get(dataset)

        if live_client is None:
            live_client = nautilus_pyo3.DatabentoLiveClient(
                key=self._live_api_key,
                dataset=dataset,
                publishers_filepath=str(PUBLISHERS_FILEPATH),
                use_exchange_as_venue=self._use_exchange_as_venue,
                bars_timestamp_on_close=self._bars_timestamp_on_close,
                reconnect_timeout_mins=self._reconnect_timeout_mins,
            )
            self._live_clients_mbo[dataset] = live_client

        return live_client

    async def _check_live_client_started(
        self,
        dataset: Dataset,
        live_client: nautilus_pyo3.DatabentoLiveClient,
    ) -> None:
        if not self._has_subscribed.get(dataset):
            self._log.debug(f"Starting {dataset} live client", LogColor.MAGENTA)
            future = asyncio.ensure_future(
                live_client.start(
                    callback=self._handle_msg,
                    callback_pyo3=self._handle_msg_pyo3,  # Imbalance and Statistics messages
                ),
            )
            future.add_done_callback(self._log_future_exception_callback)
            self._live_client_futures.add(future)
            self._has_subscribed[dataset] = True
            self._log.info(f"Started {dataset} live feed", LogColor.BLUE)

    def _send_all_instruments_to_data_engine(self) -> None:
        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)

    async def _ensure_subscribed_for_instrument(self, instrument_id: InstrumentId) -> None:
        try:
            dataset: Dataset = self._loader.get_dataset_for_venue(instrument_id.venue)
            subscribed_instruments = self._instrument_ids[dataset]

            if instrument_id in subscribed_instruments:
                return

            self._instrument_ids[dataset].add(instrument_id)
            subscribe = SubscribeInstrument(
                instrument_id=instrument_id,
                client_id=None,
                venue=instrument_id.venue,
                command_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
            )
            await self._subscribe_instrument(subscribe)
        except asyncio.CancelledError:
            self._log.warning(
                "Canceled task 'ensure_subscribed_for_instrument'",
            )

    async def _ensure_subscribed_for_instruments(
        self,
        dataset: Dataset,
        instrument_ids: list[InstrumentId],
    ) -> None:
        """
        Ensure all instruments are subscribed for definitions in a single batch.
        """
        try:
            subscribed_instruments = self._instrument_ids[dataset]

            # Filter to only new instruments
            new_instrument_ids = [
                iid for iid in instrument_ids if iid not in subscribed_instruments
            ]

            if not new_instrument_ids:
                return

            # Mark all as subscribed
            for instrument_id in new_instrument_ids:
                self._instrument_ids[dataset].add(instrument_id)

            # Subscribe in batch
            await self._subscribe_instrument_ids(dataset, new_instrument_ids)
        except asyncio.CancelledError:
            self._log.warning("Canceled task 'ensure_subscribed_for_instruments'")

    # TODO: Temporary solution until first-class batch subscription commands
    def _resolve_instrument_ids_and_dataset(
        self,
        command,
    ) -> tuple[list[InstrumentId], Dataset] | None:
        instrument_ids_param: list[InstrumentId] | None = command.params.get(
            "instrument_ids",
        )
        if instrument_ids_param:
            instrument_ids = instrument_ids_param
        else:
            instrument_ids = [command.instrument_id]

        datasets = {
            self._loader.get_dataset_for_venue(instrument_id.venue)
            for instrument_id in instrument_ids
        }
        if len(datasets) > 1:
            self._log.error(
                f"Cannot subscribe to instruments from multiple datasets: {datasets}. "
                f"All subscriptions must belong to the same dataset.",
            )
            return None

        return instrument_ids, datasets.pop()

    async def _get_dataset_range(
        self,
        dataset: Dataset,
    ) -> tuple[pd.Timestamp | None, pd.Timestamp]:
        # Check and cache dataset available range
        while dataset in self._dataset_ranges_requested:
            await asyncio.sleep(0.1)

        available_range = self._dataset_ranges.get(dataset)

        if available_range:
            return available_range

        self._dataset_ranges_requested.add(dataset)

        try:
            self._log.info(f"Requesting dataset range for {dataset}...", LogColor.BLUE)
            response = await self._http_client.get_dataset_range(dataset)
            start_str = response["start"].replace("+00:00:00", "")
            end_str = response["end"].replace("+00:00:00", "")

            available_start = pd.to_datetime(start_str, utc=True)
            available_end = pd.to_datetime(end_str, utc=True)
            self._dataset_ranges[dataset] = (available_start, available_end)

            self._log.info(
                f"Dataset {dataset} available end {available_end.date()}",
                LogColor.BLUE,
            )

            return available_start, available_end
        except asyncio.CancelledError:
            self._log.warning("Canceled task 'get_dataset_range'")

            return (None, pd.Timestamp.utcnow())
        except Exception as e:  # More specific exception
            self._log.exception("Error requesting dataset range", e)

            return (None, pd.Timestamp.utcnow())
        finally:
            self._dataset_ranges_requested.discard(dataset)

    # -- OVERRIDES ----------------------------------------------------------------------------

    def subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        if command.book_type != BookType.L3_MBO:
            raise NotImplementedError("Use BookType.L3_MBO for Databento")

        self.create_task(
            self._subscribe_order_book_deltas(command),
            log_msg=f"subscribe: order_book_deltas {command.instrument_id}",
            actions=lambda: self._add_subscription_order_book_deltas(command.instrument_id),
        )

    def subscribe_order_book_snapshots(self, command: SubscribeOrderBook) -> None:
        # Register all instrument_ids from params for bulk subscriptions
        instrument_ids: list[InstrumentId] | None = command.params.get("instrument_ids")
        if instrument_ids:
            for instrument_id in instrument_ids:
                self._add_subscription_order_book_snapshots(instrument_id)
        else:
            self._add_subscription_order_book_snapshots(command.instrument_id)

        self.create_task(
            self._subscribe_order_book_snapshots(command),
            log_msg=f"subscribe: order_book_snapshots {command.instrument_id}",
        )

    def subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        # Register all instrument_ids from params for bulk subscriptions
        instrument_ids: list[InstrumentId] | None = command.params.get("instrument_ids")
        if instrument_ids:
            for instrument_id in instrument_ids:
                self._add_subscription_quote_ticks(instrument_id)
        else:
            self._add_subscription_quote_ticks(command.instrument_id)

        self.create_task(
            self._subscribe_quote_ticks(command),
            log_msg=f"subscribe: quote_ticks {command.instrument_id}",
            success_msg="Subscribed quotes",
            success_color=LogColor.BLUE,
        )

    def subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        # Register all instrument_ids from params for bulk subscriptions
        instrument_ids: list[InstrumentId] | None = command.params.get("instrument_ids")
        if instrument_ids:
            for instrument_id in instrument_ids:
                self._add_subscription_trade_ticks(instrument_id)
        else:
            self._add_subscription_trade_ticks(command.instrument_id)

        self.create_task(
            self._subscribe_trade_ticks(command),
            log_msg=f"subscribe: trade_ticks {command.instrument_id}",
            success_msg="Subscribed trades",
            success_color=LogColor.BLUE,
        )

    def subscribe_bars(self, command: SubscribeBars) -> None:
        # Register all bar_types from params for bulk subscriptions
        bar_types: list | None = command.params.get("bar_types")
        if bar_types:
            for bar_type in bar_types:
                self._add_subscription_bars(bar_type)
        else:
            self._add_subscription_bars(command.bar_type)

        self.create_task(
            self._subscribe_bars(command),
            log_msg=f"subscribe: bars {command.bar_type}",
            success_msg="Subscribed bars",
            success_color=LogColor.BLUE,
        )

    def subscribe_instrument_status(self, command: SubscribeInstrumentStatus) -> None:
        # Register all instrument_ids from params for bulk subscriptions
        instrument_ids: list[InstrumentId] | None = command.params.get("instrument_ids")
        if instrument_ids:
            for instrument_id in instrument_ids:
                self._add_subscription_instrument_status(instrument_id)
        else:
            self._add_subscription_instrument_status(command.instrument_id)

        self.create_task(
            self._subscribe_instrument_status(command),
            log_msg=f"subscribe: instrument_status {command.instrument_id}",
            success_msg="Subscribed instrument status",
            success_color=LogColor.BLUE,
        )

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    async def _subscribe(self, command: SubscribeData) -> None:
        if command.data_type.type == DatabentoImbalance:
            await self._subscribe_imbalance(command.data_type)
        elif command.data_type.type == DatabentoStatistics:
            await self._subscribe_statistics(command.data_type)
        else:
            raise NotImplementedError(
                f"Cannot subscribe to {command.data_type.type} (not implemented).",
            )

    async def _subscribe_imbalance(self, data_type: DataType) -> None:
        try:
            # TODO: Create `DatabentoTimeSeriesParams`
            instrument_id: InstrumentId = data_type.metadata["instrument_id"]
            dataset: Dataset = self._loader.get_dataset_for_venue(instrument_id.venue)
            live_client = self._get_live_client(dataset)
            live_client.subscribe(
                schema=DatabentoSchema.IMBALANCE.value,
                instrument_ids=[instrument_id_to_pyo3(instrument_id)],
            )
            await self._check_live_client_started(dataset, live_client)
        except asyncio.CancelledError:
            self._log.warning("Canceled task 'subscribe_imbalance'")

    async def _subscribe_statistics(self, data_type: DataType) -> None:
        try:
            # TODO: Create `DatabentoTimeSeriesParams`
            instrument_id: InstrumentId = data_type.metadata["instrument_id"]
            dataset: Dataset = self._loader.get_dataset_for_venue(instrument_id.venue)
            live_client = self._get_live_client(dataset)
            live_client.subscribe(
                schema=DatabentoSchema.STATISTICS.value,
                instrument_ids=[instrument_id_to_pyo3(instrument_id)],
            )
            await self._check_live_client_started(dataset, live_client)
        except asyncio.CancelledError:
            self._log.warning("Canceled task 'subscribe_statistics'")

    async def _subscribe_instruments(self, command: SubscribeInstruments) -> None:
        # Replace method in child class, for exchange specific data types.
        raise NotImplementedError("Cannot subscribe to all instruments (not currently supported).")

    async def _subscribe_instrument(self, command: SubscribeInstrument) -> None:
        try:
            dataset: Dataset = self._loader.get_dataset_for_venue(command.instrument_id.venue)
            start: int | None = command.params.get("start_ns")

            live_client = self._get_live_client(dataset)
            live_client.subscribe(
                schema=DatabentoSchema.DEFINITION.value,
                instrument_ids=[instrument_id_to_pyo3(command.instrument_id)],
                start=start,
            )
            await self._check_live_client_started(dataset, live_client)
        except asyncio.CancelledError:
            self._log.warning("Canceled task 'subscribe_instrument'")

    async def _subscribe_parent_symbols(
        self,
        dataset: Dataset,
        parent_symbols: set[InstrumentId],
    ) -> None:
        try:
            live_client = self._get_live_client(dataset)
            live_client.subscribe(
                schema=DatabentoSchema.DEFINITION.value,
                instrument_ids=sorted(  # type: ignore[type-var]
                    [instrument_id_to_pyo3(instrument_id) for instrument_id in parent_symbols],
                ),
                stype_in="parent",
            )
            await self._check_live_client_started(dataset, live_client)
        except asyncio.CancelledError:
            self._log.warning("Canceled task 'subscribe_parent_symbols'")

    async def _subscribe_instrument_ids(
        self,
        dataset: Dataset,
        instrument_ids: list[InstrumentId],
    ) -> None:
        try:
            live_client = self._get_live_client(dataset)
            live_client.subscribe(
                schema=DatabentoSchema.DEFINITION.value,
                instrument_ids=[
                    instrument_id_to_pyo3(instrument_id) for instrument_id in instrument_ids
                ],
            )
            await self._check_live_client_started(dataset, live_client)
        except asyncio.CancelledError:
            self._log.warning("Canceled task 'subscribe_instrument_ids'")

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        try:
            if command.book_type != BookType.L3_MBO:
                raise NotImplementedError

            if command.depth:  # Can be None or 0 (full depth)
                self._log.error(
                    f"Cannot subscribe to order book deltas with specific depth of {command.depth} "
                    "(do not specify depth when subscribing, must be full depth)",
                )
                return

            dataset: Dataset = self._loader.get_dataset_for_venue(command.instrument_id.venue)

            if self._is_buffering_mbo_subscriptions:
                self._log.debug(
                    f"Buffering MBO/L3 subscription for {command.instrument_id}",
                    LogColor.MAGENTA,
                )
                self._buffered_mbo_subscriptions[dataset].append(command.instrument_id)
                return

            if self._live_clients_mbo.get(dataset) is not None:
                self._log.error(
                    f"Cannot subscribe to order book deltas for {command.instrument_id}, "
                    "MBO/L3 feed already started",
                )
                return

            await self._subscribe_order_book_deltas_batch([command.instrument_id])
        except asyncio.CancelledError:
            self._log.warning("Canceled task 'subscribe_order_book_deltas'")

    async def _subscribe_order_book_deltas_batch(
        self,
        instrument_ids: list[InstrumentId],
    ) -> None:
        try:
            if not instrument_ids:
                self._log.warning(
                    "No subscriptions for order book deltas (`instrument_ids` was empty)",
                )
                return

            dataset: Dataset = self._loader.get_dataset_for_venue(instrument_ids[0].venue)
            live_client = self._get_live_client_mbo(dataset)

            if dataset == "GLBX.MDP3":
                start = None
                snapshot = True
                detail_str = " with snapshot"
            else:
                start = 0
                snapshot = False
                detail_str = " with start=0 replay"

            ids_str = ",".join([i.value for i in instrument_ids])
            self._log.info(f"Subscribing to MBO/L3 for {ids_str}{detail_str}", LogColor.BLUE)

            live_client.subscribe(
                schema=DatabentoSchema.MBO.value,
                instrument_ids=[
                    instrument_id_to_pyo3(instrument_id) for instrument_id in instrument_ids
                ],
                start=start,
                snapshot=snapshot,
            )

            # Add trade tick subscriptions for all instruments (MBO data includes trades)
            for instrument_id in instrument_ids:
                self._trade_tick_subscriptions.add(instrument_id)

            future = asyncio.ensure_future(
                live_client.start(
                    callback=self._handle_msg,
                    callback_pyo3=self._handle_msg_pyo3,  # Imbalance and Statistics messages
                ),
            )
            future.add_done_callback(self._log_future_exception_callback)
            self._live_client_futures.add(future)
        except asyncio.CancelledError:
            self._log.warning(
                "Canceled task 'subscribe_order_book_deltas_batch'",
            )

    async def _subscribe_order_book_snapshots(self, command: SubscribeOrderBook) -> None:
        try:
            match command.depth:
                case 1:
                    schema = DatabentoSchema.MBP_1.value
                case 10:
                    schema = DatabentoSchema.MBP_10.value
                case _:
                    self._log.error(
                        f"Cannot subscribe for order book snapshots of depth {command.depth}, use either 1 or 10",
                    )
                    return

            result = self._resolve_instrument_ids_and_dataset(command)
            if result is None:
                return

            instrument_ids, dataset = result

            await self._ensure_subscribed_for_instruments(dataset, instrument_ids)

            live_client = self._get_live_client(dataset)
            live_client.subscribe(
                schema=schema,
                instrument_ids=[
                    instrument_id_to_pyo3(instrument_id) for instrument_id in instrument_ids
                ],
            )
            await self._check_live_client_started(dataset, live_client)
        except asyncio.CancelledError:
            self._log.warning("Canceled task 'subscribe_order_book_snapshots'")

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        try:
            result = self._resolve_instrument_ids_and_dataset(command)
            if result is None:
                return

            instrument_ids, dataset = result

            # Allowed schema values: mbp-1, bbo-1s, bbo-1m, cmbp-1, cbbo-1s, cbbo-1m, tbbo, tcbbo
            schema: str | None = command.params.get("schema")
            if schema is None or schema not in [
                DatabentoSchema.MBP_1.value,
                DatabentoSchema.BBO_1S.value,
                DatabentoSchema.BBO_1M.value,
                DatabentoSchema.CMBP_1.value,
                DatabentoSchema.CBBO_1S.value,
                DatabentoSchema.CBBO_1M.value,
                DatabentoSchema.TBBO.value,
                DatabentoSchema.TCBBO.value,
            ]:
                self._log.warning(
                    f"Schema {schema} not supported for quotes. Defaulting to {DatabentoSchema.MBP_1}",
                )
                schema = DatabentoSchema.MBP_1.value

            start: int | None = command.params.get("start_ns")

            await self._ensure_subscribed_for_instruments(dataset, instrument_ids)

            self._log.info(
                f"Subscribing to quotes (schema: {schema}) from dataset {dataset} for {len(instrument_ids)} instrument ids:",
                LogColor.BLUE,
            )
            for i, instrument_id in enumerate(instrument_ids):
                self._log.info(f"  [{i}] {instrument_id}", LogColor.BLUE)

            # Subscribe
            live_client = self._get_live_client(dataset)
            live_client.subscribe(
                schema=schema,
                instrument_ids=[
                    instrument_id_to_pyo3(instrument_id) for instrument_id in instrument_ids
                ],
                start=start,
            )

            # Add trade tick subscriptions for instruments (MBP-1 data includes trades)
            for instrument_id in instrument_ids:
                self._trade_tick_subscriptions.add(instrument_id)

            await self._check_live_client_started(dataset, live_client)
        except asyncio.CancelledError:
            self._log.warning("Canceled task 'subscribe_quote_ticks'")

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        try:
            result = self._resolve_instrument_ids_and_dataset(command)
            if result is None:
                return

            instrument_ids, dataset = result

            # Filter out already-subscribed instruments to save on data costs
            instrument_ids = [
                inst_id
                for inst_id in instrument_ids
                if inst_id not in self._trade_tick_subscriptions
            ]
            if not instrument_ids:
                return

            # Allowed schema values: trades, tbbo, tcbbo, mbp-1, cmbp-1
            schema: str | None = command.params.get("schema")
            if schema is None or schema not in [
                DatabentoSchema.TRADES.value,
                DatabentoSchema.TBBO.value,
                DatabentoSchema.TCBBO.value,
                DatabentoSchema.MBP_1.value,  # MBP-1 can also emit trades
                DatabentoSchema.CMBP_1.value,  # CMBP-1 can also emit trades
            ]:
                schema = DatabentoSchema.TRADES.value

            start: int | None = command.params.get("start_ns")

            await self._ensure_subscribed_for_instruments(dataset, instrument_ids)

            # Subscribe
            live_client = self._get_live_client(dataset)
            live_client.subscribe(
                schema=schema,
                instrument_ids=[
                    instrument_id_to_pyo3(instrument_id) for instrument_id in instrument_ids
                ],
                start=start,
            )
            await self._check_live_client_started(dataset, live_client)
        except asyncio.CancelledError:
            self._log.warning("Canceled task 'subscribe_trade_ticks'")

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        try:
            bar_types_param: list | None = command.params.get("bar_types")
            if bar_types_param:
                bar_types = bar_types_param
            else:
                bar_types = [command.bar_type]

            start: int | None = command.params.get("start_ns")
            schema_override: str | None = command.params.get("schema")

            # Validate all bar_types belong to the same dataset
            datasets = {
                self._loader.get_dataset_for_venue(bar_type.instrument_id.venue)
                for bar_type in bar_types
            }
            if len(datasets) > 1:
                self._log.error(
                    f"Cannot subscribe to bar types from multiple datasets: {datasets}. "
                    f"All subscriptions must belong to the same dataset.",
                )
                return

            dataset = datasets.pop()

            # Determine schema for all bar_types (must be the same)
            schemas = set()
            instrument_ids = []
            for bar_type in bar_types:
                try:
                    schema = databento_schema_from_nautilus_bar_type(bar_type)
                except ValueError as e:
                    self._log.error(f"Cannot subscribe to {bar_type}: {e}")
                    return

                # Check for schema override in params
                if schema_override:
                    if (
                        bar_type.spec.aggregation == BarAggregation.DAY
                        and schema_override == "ohlcv-eod"
                    ):
                        # Allow ohlcv-eod override for daily bars
                        schema = DatabentoSchema.OHLCV_EOD
                    else:
                        self._log.error(
                            f"Invalid schema override '{schema_override}' for bar type {bar_type}. "
                            f"Only 'ohlcv-eod' is supported for DAY aggregation bars.",
                        )
                        return

                schemas.add(schema.value)
                instrument_ids.append(bar_type.instrument_id)

            # Validate all bar_types use the same schema
            if len(schemas) > 1:
                self._log.error(
                    f"Cannot subscribe to bar types with multiple schemas: {schemas}. "
                    f"All subscriptions must use the same schema.",
                )
                return

            schema_value = schemas.pop()

            # Subscribe
            live_client = self._get_live_client(dataset)
            live_client.subscribe(
                schema=schema_value,
                instrument_ids=[
                    instrument_id_to_pyo3(instrument_id) for instrument_id in instrument_ids
                ],
                start=start,
            )
            await self._check_live_client_started(dataset, live_client)
        except asyncio.CancelledError:
            self._log.warning("Canceled task 'subscribe_bars'")

    async def _subscribe_instrument_status(self, command: SubscribeInstrumentStatus) -> None:
        try:
            result = self._resolve_instrument_ids_and_dataset(command)
            if result is None:
                return

            instrument_ids, dataset = result

            live_client = self._get_live_client(dataset)
            live_client.subscribe(
                schema=DatabentoSchema.STATUS.value,
                instrument_ids=[
                    instrument_id_to_pyo3(instrument_id) for instrument_id in instrument_ids
                ],
            )
            await self._check_live_client_started(dataset, live_client)
        except asyncio.CancelledError:
            self._log.warning("Canceled task 'subscribe_instrument_status'")

    async def _unsubscribe(self, command: UnsubscribeData) -> None:
        raise NotImplementedError(
            f"Cannot unsubscribe from {command.data_type}, unsubscribing not supported by Databento.",
        )

    async def _unsubscribe_instruments(self, command: UnsubscribeInstruments) -> None:
        raise NotImplementedError(
            "Cannot unsubscribe from all instruments, unsubscribing not supported by Databento.",
        )

    async def _unsubscribe_instrument(self, command: UnsubscribeInstrument) -> None:
        raise NotImplementedError(
            f"Cannot unsubscribe from {command.instrument_id} instrument, "
            "unsubscribing not supported by Databento.",
        )

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        raise NotImplementedError(
            f"Cannot unsubscribe from {command.instrument_id} order book deltas, "
            "unsubscribing not supported by Databento.",
        )

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        raise NotImplementedError(
            f"Cannot unsubscribe from {command.instrument_id} quotes, "
            "unsubscribing not supported by Databento.",
        )

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        raise NotImplementedError(
            f"Cannot unsubscribe from {command.instrument_id} trades, "
            "unsubscribing not supported by Databento.",
        )

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        raise NotImplementedError(
            f"Cannot unsubscribe from {command.bar_type} bars, "
            "unsubscribing not supported by Databento.",
        )

    async def _unsubscribe_instrument_status(self, command: UnsubscribeInstrumentStatus) -> None:
        raise NotImplementedError(
            f"Cannot unsubscribe from {command.instrument_id} instrument status, "
            "unsubscribing not supported by Databento.",
        )

    async def _request(self, request: RequestData) -> None:
        if request.data_type.type == InstrumentStatus:
            await self._request_instrument_status(request.data_type, request.id)
        elif request.data_type.type == DatabentoImbalance:
            await self._request_imbalance(request.data_type, request.id)
        elif request.data_type.type == DatabentoStatistics:
            await self._request_statistics(request.data_type, request.id)
        elif request.data_type.type == OrderBookDeltas:
            await self._request_order_book_deltas(request)
        elif request.data_type.type == OrderBookDepth10:
            await self._request_order_book_depth(request)
        else:
            raise NotImplementedError(
                f"Cannot request {request.data_type.type} (not implemented)",
            )

    async def _resolve_time_range_for_request(
        self,
        dataset: Dataset,
        start: pd.Timestamp | None,
        end: pd.Timestamp | None,
    ) -> tuple[pd.Timestamp, pd.Timestamp]:
        _, available_end = await self._get_dataset_range(dataset)
        original_start, original_end = start, end

        # Default end to dataset end when missing
        end = end or available_end
        if original_end is not None and end > available_end:
            end = available_end
            self._log.info(
                f"Clamped end from {original_end} to dataset end {available_end}",
                LogColor.BLUE,
            )

        # Default start to day boundary of end
        start = start or end.floor("D")

        if start > end:
            prev_start = start
            start = end
            self._log.info(
                f"Clamped start from {prev_start} to end {end} (start > end)",
                LogColor.BLUE,
            )

        if start == end and end < available_end:
            end += pd.Timedelta(1, "ns")
            self._log.info(
                "Adjusted end by +1ns to create non-empty interval",
                LogColor.BLUE,
            )
        elif start == end:
            # At dataset boundary (e.g. available_end=midnight, floor("D") gives start=end)
            start -= pd.Timedelta(1, "ns")
            self._log.info("Adjusted start by -1ns to create non-empty interval", LogColor.BLUE)

        if original_start != start or (original_end is not None and original_end != end):
            self._log.info(
                f"Resolved time range: {original_start=}, {original_end=} -> {start=}, {end=}",
                LogColor.BLUE,
            )

        return start, end

    async def _request_instrument_status(
        self,
        data_type: DataType,
        correlation_id: UUID4,
    ) -> None:
        # Check if multiple instrument_ids are provided in metadata
        instrument_ids_param: list[InstrumentId] | None = data_type.metadata.get("instrument_ids")
        if instrument_ids_param:
            instrument_ids = instrument_ids_param
        else:
            instrument_id: InstrumentId = data_type.metadata["instrument_id"]
            instrument_ids = [instrument_id]

        start = data_type.metadata.get("start")
        end = data_type.metadata.get("end")

        # Validate all instrument_ids belong to the same dataset
        datasets = {self._loader.get_dataset_for_venue(inst_id.venue) for inst_id in instrument_ids}
        if len(datasets) > 1:
            self._log.error(
                f"Cannot request instrument status from multiple datasets: {datasets}. "
                f"All requests must belong to the same dataset.",
            )
            return

        dataset = datasets.pop()
        start, end = await self._resolve_time_range_for_request(dataset, start, end)

        self._log.info(
            f"Requesting instrument status for {len(instrument_ids)} instruments: "
            f"dataset={dataset}, start={start}, end={end}",
            LogColor.BLUE,
        )
        for i, inst_id in enumerate(instrument_ids):
            self._log.info(f"  [{i}] {inst_id}", LogColor.BLUE)

        pyo3_status_list = await self._http_client.get_range_status(
            dataset=dataset,
            instrument_ids=[instrument_id_to_pyo3(inst_id) for inst_id in instrument_ids],
            start=start.value,
            end=end.value,
        )
        status = InstrumentStatus.from_pyo3_list(pyo3_status_list)
        self._handle_data_response(
            data_type=data_type,
            data=status,
            correlation_id=correlation_id,
            start=None,
            end=None,
            params=None,
        )

    async def _request_imbalance(self, data_type: DataType, correlation_id: UUID4) -> None:
        # Check if multiple instrument_ids are provided in metadata
        instrument_ids_param: list[InstrumentId] | None = data_type.metadata.get("instrument_ids")
        if instrument_ids_param:
            instrument_ids = instrument_ids_param
        else:
            instrument_id: InstrumentId = data_type.metadata["instrument_id"]
            instrument_ids = [instrument_id]

        start = data_type.metadata.get("start")
        end = data_type.metadata.get("end")

        # Validate all instrument_ids belong to the same dataset
        datasets = {self._loader.get_dataset_for_venue(inst_id.venue) for inst_id in instrument_ids}
        if len(datasets) > 1:
            self._log.error(
                f"Cannot request imbalance from multiple datasets: {datasets}. "
                f"All requests must belong to the same dataset.",
            )
            return

        dataset = datasets.pop()
        start, end = await self._resolve_time_range_for_request(dataset, start, end)

        self._log.info(
            f"Requesting imbalance for {len(instrument_ids)} instruments: "
            f"dataset={dataset}, start={start}, end={end}",
            LogColor.BLUE,
        )
        for i, inst_id in enumerate(instrument_ids):
            self._log.info(f"  [{i}] {inst_id}", LogColor.BLUE)

        pyo3_imbalances = await self._http_client.get_range_imbalance(
            dataset=dataset,
            instrument_ids=[instrument_id_to_pyo3(inst_id) for inst_id in instrument_ids],
            start=start.value,
            end=end.value,
        )
        self._handle_data_response(
            data_type=data_type,
            data=pyo3_imbalances,
            correlation_id=correlation_id,
            start=None,
            end=None,
            params=None,
        )

    async def _request_statistics(self, data_type: DataType, correlation_id: UUID4) -> None:
        # Check if multiple instrument_ids are provided in metadata
        instrument_ids_param: list[InstrumentId] | None = data_type.metadata.get("instrument_ids")
        if instrument_ids_param:
            instrument_ids = instrument_ids_param
        else:
            instrument_id: InstrumentId = data_type.metadata["instrument_id"]
            instrument_ids = [instrument_id]

        start = data_type.metadata.get("start")
        end = data_type.metadata.get("end")

        # Validate all instrument_ids belong to the same dataset
        datasets = {self._loader.get_dataset_for_venue(inst_id.venue) for inst_id in instrument_ids}
        if len(datasets) > 1:
            self._log.error(
                f"Cannot request statistics from multiple datasets: {datasets}. "
                f"All requests must belong to the same dataset.",
            )
            return

        dataset = datasets.pop()
        start, end = await self._resolve_time_range_for_request(dataset, start, end)

        self._log.info(
            f"Requesting statistics for {len(instrument_ids)} instruments: "
            f"dataset={dataset}, start={start}, end={end}",
            LogColor.BLUE,
        )
        for i, inst_id in enumerate(instrument_ids):
            self._log.info(f"  [{i}] {inst_id}", LogColor.BLUE)

        pyo3_statistics = await self._http_client.get_range_statistics(
            dataset=dataset,
            instrument_ids=[instrument_id_to_pyo3(inst_id) for inst_id in instrument_ids],
            start=start.value,
            end=end.value,
        )
        self._handle_data_response(
            data_type=data_type,
            data=pyo3_statistics,
            correlation_id=correlation_id,
            start=None,
            end=None,
            params=None,
        )

    async def _request_instrument(self, request: RequestInstrument) -> None:
        # Check if multiple instrument_ids are provided in params
        instrument_ids_param: list[InstrumentId] | None = request.params.get("instrument_ids")
        if instrument_ids_param:
            instrument_ids = instrument_ids_param
        else:
            instrument_ids = [request.instrument_id]

        # Validate all instrument_ids belong to the same dataset
        datasets = {
            self._loader.get_dataset_for_venue(instrument_id.venue)
            for instrument_id in instrument_ids
        }
        if len(datasets) > 1:
            self._log.error(
                f"Cannot request instruments from multiple datasets: {datasets}. "
                f"All requests must belong to the same dataset.",
            )
            return

        dataset = datasets.pop()
        start, end = await self._resolve_time_range_for_request(dataset, request.start, request.end)

        self._log.info(
            f"Requesting instrument definitions for {len(instrument_ids)} instruments: "
            f"dataset={dataset}, start={start}, end={end}",
            LogColor.BLUE,
        )
        for i, instrument_id in enumerate(instrument_ids):
            self._log.info(f"  [{i}] {instrument_id}", LogColor.BLUE)

        pyo3_instruments = await self._http_client.get_range_instruments(
            dataset=dataset,
            instrument_ids=[instrument_id_to_pyo3(inst_id) for inst_id in instrument_ids],
            start=start.value,
            end=end.value,
        )
        instruments = instruments_from_pyo3(pyo3_instruments)

        if not instruments:
            self._log.warning(
                f"No instruments found for request: {instrument_ids=}, {request.id=}",
            )
            return

        # Handle each instrument
        for instrument in instruments:
            self._handle_instrument(
                instrument,
                correlation_id=request.id,
                start=request.start,
                end=request.end,
                params=request.params,
            )

    async def _request_instruments(self, request: RequestInstruments) -> None:
        dataset: Dataset = self._loader.get_dataset_for_venue(request.venue)
        start, end = await self._resolve_time_range_for_request(dataset, request.start, request.end)

        self._log.info(
            f"Requesting {request.venue} instrument definitions: "
            f"dataset={dataset}, start={start}, end={end}",
            LogColor.BLUE,
        )

        # parent_symbols can be equal to ["ES.OPT", "ES.FUT"] for example in order to not query all instruments of an exchange
        parent_symbols = request.params.get("parent_symbols") or [ALL_SYMBOLS]
        pyo3_instrument_ids = [
            instrument_id_to_pyo3(InstrumentId.from_str(f"{symbol}.{request.venue}"))
            for symbol in parent_symbols
        ]
        pyo3_instruments = await self._http_client.get_range_instruments(
            dataset=dataset,
            instrument_ids=pyo3_instrument_ids,
            start=start.value,
            end=end.value,
        )
        instruments = instruments_from_pyo3(pyo3_instruments)

        self._handle_instruments(
            request.venue,
            instruments,
            correlation_id=request.id,
            start=request.start,
            end=request.end,
            params=request.params,
        )

    async def _request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        # Check if multiple instrument_ids are provided in params
        instrument_ids_param: list[InstrumentId] | None = request.params.get("instrument_ids")
        if instrument_ids_param:
            instrument_ids = instrument_ids_param
        else:
            instrument_ids = [request.instrument_id]

        # Validate all instrument_ids belong to the same dataset
        datasets = {
            self._loader.get_dataset_for_venue(instrument_id.venue)
            for instrument_id in instrument_ids
        }
        if len(datasets) > 1:
            self._log.error(
                f"Cannot request quotes for instruments from multiple datasets: {datasets}. "
                f"All requests must belong to the same dataset.",
            )
            return

        dataset = datasets.pop()
        start, end = await self._resolve_time_range_for_request(dataset, request.start, request.end)

        if request.limit > 0:
            self._log.warning(
                f"Ignoring limit {request.limit} because it's applied from the start (instead of the end)",
            )

        self._log.info(
            f"Requesting quotes for {len(instrument_ids)} instruments: dataset={dataset}, start={start}, end={end}",
            LogColor.BLUE,
        )
        for i, instrument_id in enumerate(instrument_ids):
            self._log.info(f"  [{i}] {instrument_id}", LogColor.BLUE)

        # Allowed schema values: mbp-1, bbo-1s, bbo-1m, cmbp-1, cbbo-1s, cbbo-1m, tbbo, tcbbo
        schema: str | None = request.params.get("schema")

        if schema is None or schema not in [
            DatabentoSchema.MBP_1.value,
            DatabentoSchema.BBO_1S.value,
            DatabentoSchema.BBO_1M.value,
            DatabentoSchema.CMBP_1.value,
            DatabentoSchema.CBBO_1S.value,
            DatabentoSchema.CBBO_1M.value,
            DatabentoSchema.TBBO.value,
            DatabentoSchema.TCBBO.value,
        ]:
            schema = DatabentoSchema.MBP_1.value

        pyo3_quotes = await self._http_client.get_range_quotes(
            dataset=dataset,
            instrument_ids=[instrument_id_to_pyo3(inst_id) for inst_id in instrument_ids],
            start=start.value,
            end=end.value,
            schema=schema,
        )
        quotes = QuoteTick.from_pyo3_list(pyo3_quotes)

        self._handle_quote_ticks(
            request.instrument_id,
            quotes,
            correlation_id=request.id,
            start=request.start,
            end=request.end,
            params=request.params,
        )

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        # Check if multiple instrument_ids are provided in params
        instrument_ids_param: list[InstrumentId] | None = request.params.get("instrument_ids")
        if instrument_ids_param:
            instrument_ids = instrument_ids_param
        else:
            instrument_ids = [request.instrument_id]

        # Validate all instrument_ids belong to the same dataset
        datasets = {
            self._loader.get_dataset_for_venue(instrument_id.venue)
            for instrument_id in instrument_ids
        }
        if len(datasets) > 1:
            self._log.error(
                f"Cannot request trades for instruments from multiple datasets: {datasets}. "
                f"All requests must belong to the same dataset.",
            )
            return

        dataset = datasets.pop()
        start, end = await self._resolve_time_range_for_request(dataset, request.start, request.end)

        if request.limit > 0:
            self._log.warning(
                f"Ignoring limit {request.limit} because it's applied from the start (instead of the end)",
            )

        self._log.info(
            f"Requesting trades for {len(instrument_ids)} instruments: dataset={dataset}, start={start}, end={end}",
            LogColor.BLUE,
        )
        for i, instrument_id in enumerate(instrument_ids):
            self._log.info(f"  [{i}] {instrument_id}", LogColor.BLUE)

        pyo3_trades = await self._http_client.get_range_trades(
            dataset=dataset,
            instrument_ids=[instrument_id_to_pyo3(inst_id) for inst_id in instrument_ids],
            start=start.value,
            end=end.value,
        )
        trades = TradeTick.from_pyo3_list(pyo3_trades)

        self._handle_trade_ticks(
            request.instrument_id,
            trades,
            correlation_id=request.id,
            start=request.start,
            end=request.end,
            params=request.params,
        )

    async def _request_bars(self, request: RequestBars) -> None:
        # Check if multiple bar_types are provided in params
        bar_types_param: list | None = request.params.get("bar_types")
        if bar_types_param:
            bar_types = bar_types_param
        else:
            bar_types = [request.bar_type]

        # Extract instrument_ids from bar_types
        instrument_ids = [bar_type.instrument_id for bar_type in bar_types]

        # Validate all bar_types belong to the same dataset
        datasets = {
            self._loader.get_dataset_for_venue(bar_type.instrument_id.venue)
            for bar_type in bar_types
        }
        if len(datasets) > 1:
            self._log.error(
                f"Cannot request bars for instruments from multiple datasets: {datasets}. "
                f"All requests must belong to the same dataset.",
            )
            return

        # Validate all bar_types use the same aggregation
        aggregations = {bar_type.spec.aggregation for bar_type in bar_types}
        if len(aggregations) > 1:
            self._log.error(
                f"Cannot request bars with multiple aggregations: {aggregations}. "
                f"All bar types must use the same aggregation.",
            )
            return

        dataset = datasets.pop()
        aggregation = aggregations.pop()
        start, end = await self._resolve_time_range_for_request(dataset, request.start, request.end)

        if request.limit > 0:
            self._log.warning(
                f"Ignoring limit {request.limit} because it's applied from the start (instead of the end)",
            )

        self._log.info(
            f"Requesting 1 {bar_aggregation_to_str(aggregation)} bars for {len(instrument_ids)} instruments: "
            f"dataset={dataset}, start={start}, end={end}",
            LogColor.BLUE,
        )
        for i, instrument_id in enumerate(instrument_ids):
            self._log.info(f"  [{i}] {instrument_id}", LogColor.BLUE)

        pyo3_bars = await self._http_client.get_range_bars(
            dataset=dataset,
            instrument_ids=[instrument_id_to_pyo3(inst_id) for inst_id in instrument_ids],
            aggregation=nautilus_pyo3.BarAggregation(
                bar_aggregation_to_str(aggregation),
            ),
            start=start.value,
            end=end.value,
            timestamp_on_close=self._bars_timestamp_on_close,
        )
        bars = Bar.from_pyo3_list(pyo3_bars)

        self._handle_bars(
            bar_type=request.bar_type,
            bars=bars,
            correlation_id=request.id,
            start=request.start,
            end=request.end,
            params=request.params,
        )

    async def _request_order_book_depth(self, request: RequestOrderBookDepth) -> None:
        # Check if multiple instrument_ids are provided in params
        instrument_ids_param: list[InstrumentId] | None = request.params.get("instrument_ids")
        if instrument_ids_param:
            instrument_ids = instrument_ids_param
        else:
            instrument_ids = [request.instrument_id]

        # Validate all instrument_ids belong to the same dataset
        datasets = {
            self._loader.get_dataset_for_venue(instrument_id.venue)
            for instrument_id in instrument_ids
        }
        if len(datasets) > 1:
            self._log.error(
                f"Cannot request order book depths for instruments from multiple datasets: {datasets}. "
                f"All requests must belong to the same dataset.",
            )
            return

        dataset = datasets.pop()
        start, end = await self._resolve_time_range_for_request(dataset, request.start, request.end)

        if request.limit > 0:
            self._log.warning(
                f"Databento does not support `limit` parameter for order book depths, "
                f"ignoring limit={request.limit}",
            )

        self._log.info(
            f"Requesting order book depth data for {len(instrument_ids)} instruments: "
            f"depth={request.depth}, start={start}, end={end}",
            LogColor.BLUE,
        )
        for i, instrument_id in enumerate(instrument_ids):
            self._log.info(f"  [{i}] {instrument_id}", LogColor.BLUE)

        pyo3_depths = await self._http_client.get_order_book_depth10(
            dataset=dataset,
            instrument_ids=[instrument_id_to_pyo3(inst_id) for inst_id in instrument_ids],
            start=start.value,
            end=end.value,
            depth=request.depth,
        )
        depths = OrderBookDepth10.from_pyo3_list(pyo3_depths)

        self._handle_order_book_depths(
            instrument_id=request.instrument_id,
            depths=depths,
            correlation_id=request.id,
            start=request.start,
            end=request.end,
            params=request.params,
        )

    async def _request_order_book_deltas(self, request: RequestOrderBookDeltas) -> None:
        dataset: Dataset = self._loader.get_dataset_for_venue(request.instrument_id.venue)
        start, end = await self._resolve_time_range_for_request(dataset, request.start, request.end)

        if request.limit > 0:
            self._log.warning(
                f"Databento does not support `limit` parameter for order book deltas, "
                f"ignoring limit={request.limit}",
            )

        self._log.info(
            f"Requesting {request.instrument_id} order book deltas data: start={start}, end={end}",
            LogColor.BLUE,
        )

        # Request MBO data directly from the historical API
        pyo3_deltas = await self._http_client.get_range_order_book_deltas(
            dataset=dataset,
            instrument_ids=[instrument_id_to_pyo3(request.instrument_id)],
            start=start.value,
            end=end.value,
        )
        deltas_list = OrderBookDelta.from_pyo3_list(pyo3_deltas)

        # Group deltas into OrderBookDeltas objects by sequence and F_LAST flag
        deltas: list[OrderBookDeltas] = []
        current_group: list[OrderBookDelta] = []

        for delta in deltas_list:
            current_group.append(delta)

            # Check if this is the last delta in an event (F_LAST flag)
            if delta.flags & RecordFlag.F_LAST:
                deltas.append(
                    OrderBookDeltas(
                        instrument_id=request.instrument_id,
                        deltas=current_group.copy(),
                    ),
                )
                current_group.clear()

        # Handle any remaining deltas without F_LAST flag
        if current_group:
            deltas.append(
                OrderBookDeltas(
                    instrument_id=request.instrument_id,
                    deltas=current_group,
                ),
            )

        self._handle_order_book_deltas(
            instrument_id=request.instrument_id,
            deltas=deltas,
            correlation_id=request.id,
            start=request.start,
            end=request.end,
            params=request.params,
        )

    def _handle_msg_pyo3(
        self,
        record: object,
    ) -> None:
        if isinstance(record, DatabentoSubscriptionAck):
            self._handle_subscription_ack(record)
            return

        if isinstance(record, nautilus_pyo3.InstrumentStatus):
            data = InstrumentStatus.from_pyo3(record)
        elif isinstance(record, DatabentoImbalance):
            instrument_id = InstrumentId.from_str(record.instrument_id.value)
            data = DataType(DatabentoImbalance, metadata={"instrument_id": instrument_id})
        elif isinstance(record, DatabentoStatistics):
            instrument_id = InstrumentId.from_str(record.instrument_id.value)
            data = DataType(DatabentoStatistics, metadata={"instrument_id": instrument_id})
        else:
            raise RuntimeError(f"Cannot handle pyo3 record `{record!r}`")

        self._handle_data(data)

    def _handle_subscription_ack(self, ack: DatabentoSubscriptionAck) -> None:
        self._log.info(f"Subscription acknowledged: {ack.message}", LogColor.BLUE)

    def _handle_msg(
        self,
        pycapsule: object,
    ) -> None:
        # The capsule will fall out of scope at the end of this method,
        # and eventually be garbage collected. The contained pointer
        # to `Data` is still owned and managed by Rust.
        data = capsule_to_data(pycapsule)
        self._handle_data(data)
