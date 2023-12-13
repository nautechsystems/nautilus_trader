# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from asyncio.futures import Future
from collections.abc import Coroutine

import databento

from nautilus_trader.adapters.databento.config import DatabentoDataClientConfig
from nautilus_trader.adapters.databento.constants import DATABENTO_CLIENT_ID
from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.adapters.databento.parsing import parse_record
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import DataType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import InstrumentId


class DatabentoDataClient(LiveMarketDataClient):
    """
    Provides a data client for the `Databento` API.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    http_client : databento.Historical
        The binance HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.
    config : DatabentoDataClientConfig
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
        config: DatabentoDataClientConfig,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=DATABENTO_CLIENT_ID,
            venue=None,  # Not applicable
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        # Configuration
        self._live_api_key: str = config.api_key or http_client.key
        self._live_gateway: str | None = config.live_gateway
        self._datasets: list[str] = config.datasets or []
        self._instrument_ids: dict[str, set[InstrumentId]] = {}
        self._initial_load_timeout: float | None = config.initial_load_timeout

        # Clients
        self._http_client: databento.Historical = http_client
        self._live_clients: dict[str, databento.Live] = {}
        self._has_subscribed: dict[str, bool] = {}
        self._loader = DatabentoDataLoader()

        # Cache instrument index
        for instrument_id in config.instrument_ids or []:
            dataset: str = self._loader.get_dataset_for_venue(instrument_id.venue)
            if dataset not in self._instrument_ids:
                self._instrument_ids[dataset] = set()
            self._instrument_ids[dataset].add(instrument_id)

    async def _connect(self) -> None:
        if not self._instrument_ids:
            return  # Nothing else to do yet

        self._log.info("Initializing instruments...")

        tasks: list[Future] = []
        for dataset, instrument_ids in self._instrument_ids.items():
            task = self._loop.run_in_executor(
                None,
                self._load_instrument_ids,
                dataset,
                sorted(instrument_ids),
            )
            tasks.append(task)

        try:
            if self._initial_load_timeout:
                await asyncio.wait_for(asyncio.gather(*tasks), timeout=self._initial_load_timeout)
            else:
                await asyncio.gather(*tasks)
        except asyncio.TimeoutError:
            self._log.warning("Timeout waiting for instruments...")

    async def _disconnect(self) -> None:
        tasks: list[Coroutine] = []
        for dataset, client in self._live_clients.items():
            self._log.info(f"Stopping {dataset} live feed...", LogColor.BLUE)
            task = client.wait_for_close(timeout=2.0)
            tasks.append(task)

        await asyncio.gather(*tasks)

    def _get_live_client(self, dataset: str) -> databento.Live:
        client = self._live_clients.get(dataset)

        if client is None:
            client = databento.Live(key=self._live_api_key, gateway=self._live_gateway)
            client.add_callback(self._handle_record)
            self._live_clients[dataset] = client

        return client

    def _check_live_client_started(self, dataset: str, live_client: databento.Live) -> None:
        if not self._has_subscribed.get(dataset):
            self._log.debug(f"Starting live client for {dataset}...", LogColor.MAGENTA)
            live_client.start()
            self._has_subscribed[dataset] = True
            self._log.info(f"Started {dataset} live feed.", LogColor.BLUE)

    async def _ensure_subscribed_for_instrument(self, instrument_id: InstrumentId) -> None:
        dataset: str = self._loader.get_dataset_for_venue(instrument_id.venue)
        subscribed_instruments = self._instrument_ids.get(dataset)
        if not subscribed_instruments:
            subscribed_instruments = set()
            self._instrument_ids[dataset] = subscribed_instruments

        if instrument_id in subscribed_instruments:
            return

        self._instrument_ids[dataset].add(instrument_id)
        await self._subscribe_instrument(instrument_id)

    def _load_instrument_ids(self, dataset: str, instrument_ids: list[InstrumentId]) -> None:
        instrument_ids_to_decode = set(instrument_ids)

        # Use fresh live data client for a one off initial instruments load
        live_client = databento.Live(key=self._live_api_key, gateway=self._live_gateway)

        try:
            live_client.subscribe(
                dataset=dataset,
                schema=databento.Schema.DEFINITION,
                symbols=[i.symbol.value for i in instrument_ids],
                stype_in=databento.SType.RAW_SYMBOL,
                start=0,  # From start of current session (latest definition)
            )
            for record in live_client:
                if isinstance(record, databento.InstrumentDefMsg):
                    instrument = parse_record(record, self._loader.publishers())
                    self._handle_data(instrument)

                    instrument_ids_to_decode.discard(instrument.id)
                    if not instrument_ids_to_decode:
                        break
        finally:
            # Close the connection (we will still process all received data)
            live_client.stop()

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    async def _subscribe(self, data_type: DataType) -> None:
        # Replace method in child class, for exchange specific data types.
        raise NotImplementedError(f"Cannot subscribe to {data_type.type} (not implemented).")

    async def _subscribe_instruments(self) -> None:
        # Replace method in child class, for exchange specific data types.
        raise NotImplementedError("Cannot subscribe to all instruments (not currently supported).")

    async def _subscribe_instrument(self, instrument_id: InstrumentId) -> None:
        dataset: str = self._loader.get_dataset_for_venue(instrument_id.venue)
        live_client = self._get_live_client(dataset)
        live_client.subscribe(
            dataset=dataset,
            schema=databento.Schema.DEFINITION,
            symbols=[instrument_id.symbol.value],
        )
        self._check_live_client_started(dataset, live_client)

    async def _subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int | None = None,
        kwargs: dict | None = None,
    ) -> None:
        await self._ensure_subscribed_for_instrument(instrument_id)
        dataset: str = self._loader.get_dataset_for_venue(instrument_id.venue)
        live_client = self._get_live_client(dataset)
        live_client.subscribe(
            dataset=dataset,
            schema=databento.Schema.MBO,
            symbols=[instrument_id.symbol.value],
        )
        self._check_live_client_started(dataset, live_client)

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
                    f"Cannot subscribe for snapshots of depth {depth}, use either 1 or 10.",
                )
                return

        dataset: str = self._loader.get_dataset_for_venue(instrument_id.venue)
        live_client = self._get_live_client(dataset)
        live_client.subscribe(
            dataset=dataset,
            schema=schema,
            symbols=[instrument_id.symbol.value],
        )
        self._check_live_client_started(dataset, live_client)

    async def _subscribe_ticker(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(
            f"Cannot subscribe to {instrument_id} ticker (not supported by Databento).",
        )

    async def _subscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        await self._ensure_subscribed_for_instrument(instrument_id)

        dataset: str = self._loader.get_dataset_for_venue(instrument_id.venue)
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

        dataset: str = self._loader.get_dataset_for_venue(instrument_id.venue)
        live_client = self._get_live_client(dataset)
        live_client.subscribe(
            dataset=dataset,
            schema=databento.Schema.TRADES,
            symbols=[instrument_id.symbol.value],
        )
        self._check_live_client_started(dataset, live_client)

    async def _subscribe_bars(self, bar_type: BarType) -> None:
        PyCondition.true(bar_type.is_externally_aggregated(), "aggregation_source is not EXTERNAL")

        if not bar_type.spec.is_time_aggregated():
            self._log.error(
                f"Cannot subscribe to {bar_type}: only time bars are aggregated by Databento.",
            )
            return

        if bar_type.spec.step != 1:
            self._log.error(
                f"Cannot subscribe to {bar_type}: only a step of 1 is supported.",
            )

        await self._ensure_subscribed_for_instrument(bar_type.instrument_id)

        match bar_type.spec.bar_aggregation:
            case BarAggregation.SECOND:
                schema = databento.Schema.OHLCV_1S
            case BarAggregation.MINUTE:
                schema = databento.Schema.OHLCV_1M
            case BarAggregation.HOUR:
                schema = databento.Schema.OHLCV_1H
            case BarAggregation.DAY:
                schema = databento.Schema.OHLCV_1D
            case _:
                self._log.error(
                    f"Cannot subscribe to {bar_type}: "
                    "use either 'SECOND', 'MINTUE', 'HOUR' or 'DAY' aggregations.",
                )
                return

        dataset: str = self._loader.get_dataset_for_venue(bar_type.instrument_id.venue)
        live_client = self._get_live_client(dataset)
        live_client.subscribe(
            dataset=dataset,
            schema=schema,
            symbols=[bar_type.instrument_id.symbol.value],
        )
        self._check_live_client_started(dataset, live_client)

    async def _unsubscribe(self, data_type: DataType) -> None:
        raise NotImplementedError(
            f"Cannot unsubscribe from {data_type} (not supported by Databento).",
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

    async def _unsubscribe_ticker(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(
            f"Cannot unsubscribe from {instrument_id} ticker (not supported by Databento).",
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

    def _handle_record(self, record: databento.DBNRecord) -> None:
        self._log.info(f"Received {record}", LogColor.MAGENTA)
        data = parse_record(record, self._loader.publishers())

        if isinstance(data, tuple):
            self._handle_data(data[0])
            self._handle_data(data[1])
        else:
            self._handle_data(data)
