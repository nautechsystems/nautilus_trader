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

import datetime as dt

import databento
import pandas as pd

from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.adapters.databento.parsing import parse_record_with_metadata
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument


class DatabentoInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `Instrument` objects from Databento.

    Parameters
    ----------
    http_client : databento.Historical
        The Databento historical HTTP client for the provider.
    logger : Logger
        The logger for the provider.
    clock : LiveClock
        The clock for the provider.
    live_api_key : str, optional
        The specific API secret key for Databento live clients.
        If not provided then will use the historical HTTP client API key.
    live_gateway : str, optional
        The live gateway override for Databento live clients.
    loader : DatabentoDataLoader, optional
        The loader for the provider.
    config : InstrumentProviderConfig, optional
        The configuration for the provider.

    """

    def __init__(
        self,
        http_client: databento.Historical,
        logger: Logger,
        clock: LiveClock,
        live_api_key: str | None = None,
        live_gateway: str | None = None,
        loader: DatabentoDataLoader | None = None,
        config: InstrumentProviderConfig | None = None,
    ):
        super().__init__(
            logger=logger,
            config=config,
        )

        self._clock = clock
        self._config = config
        self._live_api_key = live_api_key or http_client.key
        self._live_gateway = live_gateway

        self._http_client = http_client
        self._live_clients: dict[str, databento.Live] = {}
        self._loader = loader or DatabentoDataLoader()

    async def load_all_async(self, filters: dict | None = None) -> None:
        raise RuntimeError(
            "requesting all instrument definitions is not currently supported, "
            "as this would mean every instrument definition for every dataset "
            "(potentially millions)",
        )

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        """
        Load the latest instrument definitions for the given instrument IDs into the
        provider by requesting the latest instrument definition messages from Databento.

        You must only request instrument definitions from one dataset at a time.
        The Databento dataset will be determined from either the filters, or the venues for the
        instrument IDs.

        Parameters
        ----------
        instrument_ids : list[InstrumentId]
            The instrument IDs to load.
        filters : dict, optional
            The optional filters for the instrument definition request.

        Raises
        ------
        ValueError
            If all venues in `instrument_ids` are not equal.

        Warnings
        --------
        Calling this method will incur a cost to your Databento account in USD.

        """
        PyCondition.not_empty(instrument_ids, "instrument_ids")

        instrument_ids_to_decode = set(instrument_ids)

        dataset = self._check_all_datasets_equal(instrument_ids)
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
                if isinstance(record, databento.SystemMsg) and record.is_heartbeat:
                    break

                if isinstance(record, databento.InstrumentDefMsg):
                    instrument = parse_record_with_metadata(
                        record,
                        publishers=self._loader.publishers,
                        ts_init=self._clock.timestamp_ns(),
                    )
                    self.add(instrument=instrument)
                    self._log.debug(f"Added instrument {instrument.id}.")

                    instrument_ids_to_decode.discard(instrument.id)
                    if not instrument_ids_to_decode:
                        break  # All requested instrument IDs now decoded
        finally:
            # Close the connection (we will still process all received data)
            live_client.stop()

    async def load_async(
        self,
        instrument_id: InstrumentId,
        filters: dict | None = None,
    ) -> None:
        """
        Load the latest instrument definition for the given instrument ID into the
        provider by requesting the latest instrument definition message from Databento.

        The Databento dataset will be determined from either the filters, or the venue for the
        instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to load.
        filters : dict, optional
            The optional filters for the instrument definition request.

        Warnings
        --------
        Calling this method will incur a cost to your Databento account in USD.

        """
        await self.load_ids_async([instrument_id])

    async def get_range(
        self,
        instrument_ids: list[InstrumentId],
        start: pd.Timestamp | dt.date | str | int,
        end: pd.Timestamp | dt.date | str | int | None = None,
        filters: dict | None = None,
    ) -> list[Instrument]:
        """
        Request a time series of instrument definitions for the given instrument IDs by
        making a `/timeseries.get_range(...)` request from Databento.

        Parameters
        ----------
        instrument_ids : list[InstrumentId]
            The instrument IDs for the request.
        start : pd.Timestamp or date or str or int
            The start datetime of the request time range (inclusive).
            Assumes UTC as timezone unless passed a tz-aware object.
            If an integer is passed, then this represents nanoseconds since the UNIX epoch.
        end : pd.Timestamp or date or str or int, optional
            The end datetime of the request time range (exclusive).
            Assumes UTC as timezone unless passed a tz-aware object.
            If an integer is passed, then this represents nanoseconds since the UNIX epoch.
            Values are forward-filled based on the resolution provided.
            Defaults to the same value as `start`.
        filters : dict, optional
            The optional filters for the instrument definition request.

        Warnings
        --------
        Calling this method will incur a cost to your Databento account in USD.

        """
        dataset = self._check_all_datasets_equal(instrument_ids)
        data = await self._http_client.timeseries.get_range_async(
            dataset=dataset,
            schema=databento.Schema.DEFINITION,
            start=start,
            end=end,
            symbols=[i.symbol.value for i in instrument_ids],
            stype_in=databento.SType.RAW_SYMBOL,
        )

        instruments: list[Instrument] = []

        for record in data:
            instrument = parse_record_with_metadata(
                record,
                publishers=self._loader.publishers,
                ts_init=self._clock.timestamp_ns(),
            )
            instruments.append(instrument)

        instruments = sorted(instruments, key=lambda x: x.ts_init)
        return instruments

    def _check_all_datasets_equal(self, instrument_ids: list[InstrumentId]) -> str:
        first_dataset = self._loader.get_dataset_for_venue(instrument_ids[0].venue)
        for instrument_id in instrument_ids:
            next_dataset = self._loader.get_dataset_for_venue(instrument_id.venue)
            if first_dataset != next_dataset:
                raise ValueError(
                    "Databento datasets for the provided `instrument_ids` were not equal, "
                    f"'{first_dataset}' vs '{next_dataset}'",
                )

        return first_dataset
