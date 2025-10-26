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

from typing import Any

from nautilus_trader.adapters.okx.constants import OKX_VENUE
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.nautilus_pyo3 import OKXContractType
from nautilus_trader.core.nautilus_pyo3 import OKXInstrumentType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import instruments_from_pyo3


class OKXInstrumentProvider(InstrumentProvider):
    """
    Provides Nautilus instrument definitions from OKX.

    Parameters
    ----------
    client : nautilus_pyo3.OKXHttpClient
        The OKX HTTP client.
    instrument_types : tuple[OKXInstrumentType, ...]
        The instrument types to load.
    contract_types : tuple[OKXContractType, ...], optional
        The contract types to load.
    config : InstrumentProviderConfig, optional
        The instrument provider configuration, by default None.

    """

    def __init__(
        self,
        client: nautilus_pyo3.OKXHttpClient,
        instrument_types: tuple[OKXInstrumentType, ...],
        contract_types: tuple[OKXContractType, ...] | None = None,
        instrument_families: tuple[str, ...] | None = None,
        config: InstrumentProviderConfig | None = None,
    ) -> None:
        super().__init__(config=config)
        self._client = client
        self._instrument_types = instrument_types
        self._contract_types = contract_types
        self._instrument_families = instrument_families
        self._log_warnings = config.log_warnings if config else True

        self._instruments_pyo3: list[nautilus_pyo3.Instrument] = []

    @property
    def instrument_types(self) -> tuple[OKXInstrumentType, ...]:
        """
        Return the OKX instrument types configured for the provider.

        Returns
        -------
        tuple[OKXInstrumentType, ...]

        """
        return self._instrument_types

    @property
    def contract_types(self) -> tuple[OKXContractType, ...] | None:
        """
        Return the OKX contract types configured for the provider.

        Returns
        -------
        tuple[OKXContractType, ...] | None

        """
        return self._contract_types

    @property
    def instrument_families(self) -> tuple[str, ...] | None:
        """
        Return the OKX instrument families configured for the provider.

        Returns
        -------
        tuple[str, ...] | None

        """
        return self._instrument_families

    def instruments_pyo3(self) -> list[Any]:
        """
        Return all OKX PyO3 instrument definitions held by the provider.

        Returns
        -------
        list[nautilus_pyo3.Instrument]

        """
        return self._instruments_pyo3

    async def load_all_async(self, filters: dict | None = None) -> None:
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        all_pyo3_instruments = []
        for instrument_type in self._instrument_types:
            # For OPTIONS, instrument families are required
            if instrument_type == OKXInstrumentType.OPTION:
                if self._instrument_families:
                    for family in self._instrument_families:
                        pyo3_instruments = await self._client.request_instruments(
                            instrument_type,
                            family,
                        )
                        all_pyo3_instruments.extend(pyo3_instruments)
                else:
                    self._log.error(
                        f"Instrument families required for {instrument_type}, but none configured",
                    )
            # SPOT and MARGIN don't support instFamily parameter
            elif instrument_type in (OKXInstrumentType.SPOT, OKXInstrumentType.MARGIN):
                pyo3_instruments = await self._client.request_instruments(instrument_type, None)
                all_pyo3_instruments.extend(pyo3_instruments)
            else:
                # SWAP and FUTURES support optional instrument families
                if self._instrument_families:
                    for family in self._instrument_families:
                        pyo3_instruments = await self._client.request_instruments(
                            instrument_type,
                            family,
                        )
                        all_pyo3_instruments.extend(pyo3_instruments)
                else:
                    pyo3_instruments = await self._client.request_instruments(instrument_type, None)
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

        self._log.info(f"Loaded {len(self._instruments)} instruments")

    async def load_ids_async(  # noqa: C901 (too complex)
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        if not instrument_ids:
            self._log.warning("No instrument IDs given for loading")
            return

        # Check all instrument IDs
        for instrument_id in instrument_ids:
            PyCondition.equal(instrument_id.venue, OKX_VENUE, "instrument_id.venue", "OKX")

        all_pyo3_instruments = []
        for instrument_type in self._instrument_types:
            # For OPTIONS, instrument families are required
            if instrument_type == OKXInstrumentType.OPTION:
                if self._instrument_families:
                    for family in self._instrument_families:
                        pyo3_instruments = await self._client.request_instruments(
                            instrument_type,
                            family,
                        )
                        all_pyo3_instruments.extend(pyo3_instruments)
                else:
                    self._log.error(
                        f"Instrument families required for {instrument_type}, but none configured",
                    )
            # SPOT and MARGIN don't support instFamily parameter
            elif instrument_type in (OKXInstrumentType.SPOT, OKXInstrumentType.MARGIN):
                pyo3_instruments = await self._client.request_instruments(instrument_type, None)
                all_pyo3_instruments.extend(pyo3_instruments)
            else:
                # SWAP and FUTURES support optional instrument families
                if self._instrument_families:
                    for family in self._instrument_families:
                        pyo3_instruments = await self._client.request_instruments(
                            instrument_type,
                            family,
                        )
                        all_pyo3_instruments.extend(pyo3_instruments)
                else:
                    pyo3_instruments = await self._client.request_instruments(instrument_type, None)
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
