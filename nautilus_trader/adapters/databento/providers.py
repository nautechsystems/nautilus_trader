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

import databento

from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.adapters.databento.parsing import parse_record
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId


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
        self._loader = DatabentoDataLoader()

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
        Load the given instrument IDs into the provider by requesting the latest
        instrument definitions from Databento.

        You can only request instrument definitions from one venue (dataset) at a time.

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

        # Check all venues are equal
        first_venue = instrument_ids[0].venue
        for instrument_id in instrument_ids:
            PyCondition.equal(
                first_venue,
                instrument_id.venue,
                "first venue",
                "instrument_id.venue",
            )

        instrument_ids_to_decode = set(instrument_ids)

        dataset = self._loader.get_dataset_for_venue(first_venue)
        live_client = self._get_live_client(dataset)

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
                self.add(instrument=instrument)
                self._log.debug(f"Added instrument {instrument.id}.")

                instrument_ids_to_decode.discard(instrument.id)
                if not instrument_ids_to_decode:
                    # Close the connection (we will still process all received data)
                    live_client.stop()

    async def load_async(
        self,
        instrument_id: InstrumentId,
        filters: dict | None = None,
    ) -> None:
        dataset = self._loader.get_dataset_for_venue(instrument_id.venue)
        live_client = self._get_live_client(dataset)

        live_client.subscribe(
            dataset=dataset,
            schema=databento.Schema.DEFINITION,
            symbols=[instrument_id.symbol.value],
            stype_in=databento.SType.RAW_SYMBOL,
            start=0,  # From start of current session (latest definition)
        )
        for record in live_client:
            print(record)
            if isinstance(record, databento.InstrumentDefMsg):
                instrument = parse_record(record, self._loader.publishers())
                self.add(instrument=instrument)
                self._log.debug(f"Added instrument {instrument.id}.")

                # Close the connection (we will still process all received data)
                live_client.stop()

    def _get_live_client(self, dataset: str) -> databento.Live:
        client = self._live_clients.get(dataset)

        if client is None:
            client = databento.Live(key=self._live_api_key, gateway=self._live_gateway)
            self._live_clients[dataset] = client

        return client
