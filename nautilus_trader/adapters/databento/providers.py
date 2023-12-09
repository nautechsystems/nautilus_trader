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
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.model.identifiers import InstrumentId


class DatabentoInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `Instrument` objects from Databento.

    Parameters
    ----------
    http_client : databento.Historical
        The historical Databento data client for the provider.
    live_client : databento.Live
        The live Databento data client for the provider.
    logger : Logger
        The logger for the provider.
    clock : LiveClock
        The clock for the provider.
    config : InstrumentProviderConfig, optional
        The configuration for the provider.

    """

    def __init__(
        self,
        http_client: databento.Historical,
        live_client: databento.Live,
        logger: Logger,
        clock: LiveClock,
        config: InstrumentProviderConfig | None = None,
    ):
        super().__init__(
            logger=logger,
            config=config,
        )

        self._clock = clock
        self._config = config

        self._http_client = http_client
        self._live_client = live_client
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
        pass

    async def load_async(
        self,
        instrument_id: InstrumentId,
        filters: dict | None = None,
    ) -> None:
        pass
