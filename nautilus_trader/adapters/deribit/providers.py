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

from typing import Any

from nautilus_trader.adapters.deribit.constants import DERIBIT_VENUE
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.nautilus_pyo3 import DeribitCurrency
from nautilus_trader.core.nautilus_pyo3 import DeribitProductType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import instruments_from_pyo3


class DeribitInstrumentProvider(InstrumentProvider):
    """
    Provides Nautilus instrument definitions from Deribit.

    Parameters
    ----------
    client : nautilus_pyo3.DeribitHttpClient
        The Deribit HTTP client.
    product_types : tuple[DeribitProductType, ...], optional
        The product types to load.
    config : InstrumentProviderConfig, optional
        The instrument provider configuration, by default None.

    """

    def __init__(
        self,
        client: nautilus_pyo3.DeribitHttpClient,
        product_types: tuple[DeribitProductType, ...] | None = None,
        config: InstrumentProviderConfig | None = None,
    ) -> None:
        super().__init__(config=config)
        self._client = client
        self._product_types = product_types
        self._log_warnings = config.log_warnings if config else True

        self._instruments_pyo3: list[nautilus_pyo3.Instrument] = []

    @property
    def product_types(self) -> tuple[DeribitProductType, ...] | None:
        """
        Return the Deribit product types configured for the provider.

        Returns
        -------
        tuple[DeribitProductType, ...] | None

        """
        return self._product_types

    def instruments_pyo3(self) -> list[Any]:
        """
        Return all Deribit PyO3 instrument definitions held by the provider.

        Returns
        -------
        list[nautilus_pyo3.Instrument]

        """
        return self._instruments_pyo3

    async def load_all_async(self, filters: dict | None = None) -> None:
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        all_pyo3_instruments = []
        if self._product_types:
            for product_type in self._product_types:
                pyo3_instruments = await self._client.request_instruments(
                    DeribitCurrency.ANY,
                    product_type,
                )
                all_pyo3_instruments.extend(pyo3_instruments)
        else:
            pyo3_instruments = await self._client.request_instruments(DeribitCurrency.ANY, None)
            all_pyo3_instruments.extend(pyo3_instruments)

        self._instruments_pyo3 = all_pyo3_instruments
        instruments = instruments_from_pyo3(all_pyo3_instruments)
        for instrument in instruments:
            self.add(instrument=instrument)

            base_currency = instrument.get_base_currency()
            if base_currency is not None:
                self.add_currency(base_currency)

            self.add_currency(instrument.quote_currency)
            self.add_currency(instrument.get_settlement_currency())

        self._log.info(f"Loaded {len(instruments)} instruments")

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        if not instrument_ids:
            self._log.warning("No instrument IDs given for loading")
            return

        # Check all instrument IDs
        for instrument_id in instrument_ids:
            PyCondition.equal(instrument_id.venue, DERIBIT_VENUE, "instrument_id.venue", "DERIBIT")

        all_pyo3_instruments = []
        if self._product_types:
            for product_type in self._product_types:
                pyo3_instruments = await self._client.request_instruments(
                    DeribitCurrency.ANY,
                    product_type,
                )
                all_pyo3_instruments.extend(pyo3_instruments)
        else:
            pyo3_instruments = await self._client.request_instruments(DeribitCurrency.ANY, None)
            all_pyo3_instruments.extend(pyo3_instruments)

        self._instruments_pyo3 = all_pyo3_instruments
        instruments = instruments_from_pyo3(all_pyo3_instruments)

        for instrument in instruments:
            if instrument.id not in instrument_ids:
                continue  # Filter instrument ID
            self.add(instrument=instrument)

            base_currency = instrument.get_base_currency()
            if base_currency is not None:
                self.add_currency(base_currency)

            self.add_currency(instrument.quote_currency)
            self.add_currency(instrument.get_settlement_currency())

    async def load_async(self, instrument_id: InstrumentId, filters: dict | None = None) -> None:
        PyCondition.not_none(instrument_id, "instrument_id")
        await self.load_ids_async([instrument_id], filters)
