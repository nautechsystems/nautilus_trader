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
import datetime as dt
from typing import Any

import pandas as pd
import pytz

from nautilus_trader.adapters.databento.constants import ALL_SYMBOLS
from nautilus_trader.adapters.databento.constants import PUBLISHERS_PATH
from nautilus_trader.adapters.databento.enums import DatabentoSchema
from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.instruments import instruments_from_pyo3


class DatabentoInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `Instrument` objects from Databento.

    Parameters
    ----------
    http_client : nautilus_pyo3.DatabentoHistoricalClient
        The Databento historical HTTP client for the provider.
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
        http_client: nautilus_pyo3.DatabentoHistoricalClient,
        clock: LiveClock,
        live_api_key: str | None = None,
        live_gateway: str | None = None,
        loader: DatabentoDataLoader | None = None,
        config: InstrumentProviderConfig | None = None,
    ):
        super().__init__(config=config)

        self._clock = clock
        self._config = config
        self._live_api_key = live_api_key or http_client.key
        self._live_gateway = live_gateway

        self._http_client = http_client
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

        instrument_ids_to_decode: set[str] = {i.value for i in instrument_ids}

        dataset = self._check_all_datasets_equal(instrument_ids)
        live_client = nautilus_pyo3.DatabentoLiveClient(
            key=self._live_api_key,
            dataset=dataset,
            publishers_path=str(PUBLISHERS_PATH),
        )

        parent_symbols = list(filters.get("parent_symbols", [])) if filters is not None else None

        pyo3_instruments = []
        success_msg = "All instruments received and decoded."

        def receive_instruments(pyo3_instrument: Any) -> None:
            pyo3_instruments.append(pyo3_instrument)
            instrument_ids_to_decode.discard(pyo3_instrument.id.value)
            if not parent_symbols and not instrument_ids_to_decode:
                raise asyncio.CancelledError(success_msg)

        live_client.subscribe(
            schema=DatabentoSchema.DEFINITION.value,
            symbols=sorted([i.symbol.value for i in instrument_ids]),
            start=0,  # From start of current week (latest definitions)
        )

        if parent_symbols:
            self._log.info(f"Requesting parent symbols {parent_symbols}.", LogColor.BLUE)
            live_client.subscribe(
                schema=DatabentoSchema.DEFINITION.value,
                stype_in="parent",
                symbols=parent_symbols,
                start=0,  # From start of current week (latest definitions)
            )

        try:
            await asyncio.wait_for(
                asyncio.ensure_future(
                    live_client.start(callback=receive_instruments, callback_pyo3=print),
                ),
                timeout=10.0,
            )
        except Exception as e:
            if success_msg in str(e):
                # Expected on decode completion, continue
                self._log.info(success_msg)
            else:
                self._log.error(repr(e))

        instruments = instruments_from_pyo3(pyo3_instruments)

        for instrument in instruments:
            self.add(instrument=instrument)
            self._log.debug(f"Added instrument {instrument.id}.")

        await asyncio.sleep(1.0)
        live_client.close()

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

        pyo3_instruments = await self._http_client.get_range_instruments(
            dataset=dataset,
            symbols=[ALL_SYMBOLS],
            start=pd.Timestamp(start, tz=pytz.utc).value,
            end=pd.Timestamp(end, tz=pytz.utc).value if end is not None else None,
        )

        instruments = instruments_from_pyo3(pyo3_instruments)

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
