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

from nautilus_trader.adapters.bybit.constants import BYBIT_MULTIPLIERS
from nautilus_trader.adapters.bybit.constants import BYBIT_VENUE
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.nautilus_pyo3 import BybitProductType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import instruments_from_pyo3


class BybitInstrumentProvider(InstrumentProvider):
    """
    Provides Nautilus instrument definitions from Bybit.

    Parameters
    ----------
    client : nautilus_pyo3.BybitHttpClient
        The Bybit HTTP client.
    product_types : tuple[BybitProductType, ...]
        The product types to load.
    config : InstrumentProviderConfig, optional
        The instrument provider configuration, by default None.

    """

    def __init__(
        self,
        client: nautilus_pyo3.BybitHttpClient,
        product_types: tuple[BybitProductType, ...],
        config: InstrumentProviderConfig | None = None,
    ) -> None:
        super().__init__(config=config)
        self._client = client
        self._product_types = product_types
        self._log_warnings = config.log_warnings if config else True

        self._instruments_pyo3: list[nautilus_pyo3.Instrument] = []

    @property
    def product_types(self) -> tuple[BybitProductType, ...]:
        """
        Return the Bybit product types configured for the provider.

        Returns
        -------
        tuple[BybitProductType, ...]

        """
        return self._product_types

    def instruments_pyo3(self) -> list[Any]:
        """
        Return all Bybit PyO3 instrument definitions held by the provider.

        Returns
        -------
        list[nautilus_pyo3.Instrument]

        """
        return self._instruments_pyo3

    def _strip_multiplier_prefix(self, code: str, symbol: str) -> str:
        """
        Strip known multiplier prefixes from Bybit currency codes.

        Bybit uses numerical prefixes in LINEAR contract symbols for low-value tokens.
        The multiplier appears in both the symbol and the coin code.
        Examples: symbol="1000000MOGUSDT", baseCoin="1000000MOG" -> "MOG"

        This prevents incorrectly stripping currencies like "1INCH" or "100X".

        Parameters
        ----------
        code : str
            The currency code to check.
        symbol : str
            The instrument symbol to validate against.

        Returns
        -------
        str
            The stripped currency code or original if no multiplier found.

        """
        for multiplier in BYBIT_MULTIPLIERS:
            prefix = str(multiplier)
            # Only strip if BOTH symbol and code start with the multiplier
            if code.startswith(prefix) and symbol.startswith(prefix):
                stripped = code[len(prefix) :]
                # Validate: remaining part should be alphabetic (3-10 chars for valid crypto tickers)
                if stripped and stripped.isalpha() and 3 <= len(stripped) <= 10:
                    self._log.debug(
                        f"Stripped multiplier prefix {multiplier} from {code}, using {stripped}",
                    )
                    return stripped

        return code

    async def load_all_async(self, filters: dict | None = None) -> None:
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        all_pyo3_instruments = []

        for product_type in self._product_types:
            pyo3_instruments = await self._client.request_instruments(product_type, None)
            all_pyo3_instruments.extend(pyo3_instruments)

        self._instruments_pyo3 = all_pyo3_instruments
        instruments = instruments_from_pyo3(all_pyo3_instruments)
        for instrument in instruments:
            self.add(instrument=instrument)

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        if not instrument_ids:
            self._log.warning("No instrument IDs given for loading")
            return

        # Validate all instrument IDs
        for instrument_id in instrument_ids:
            PyCondition.equal(instrument_id.venue, BYBIT_VENUE, "instrument_id.venue", "BYBIT")

        symbol_product_pairs: set[tuple[str, BybitProductType]] = set()

        for instrument_id in instrument_ids:
            try:
                # Parse product type from symbol (requires suffix like -SPOT, -LINEAR, etc.)
                product_type = nautilus_pyo3.bybit_product_type_from_symbol(
                    instrument_id.symbol.value,
                )
                raw_symbol = nautilus_pyo3.bybit_extract_raw_symbol(instrument_id.symbol.value)
            except ValueError:
                # Symbol lacks suffix (e.g., options without -OPTION), try all configured types
                raw_symbol = instrument_id.symbol.value
                for product_type in self._product_types:
                    symbol_product_pairs.add((raw_symbol, product_type))
                continue

            if product_type not in self._product_types:
                raise ValueError(
                    f"Instrument {instrument_id} has product type {product_type} "
                    f"which is not in configured product types {self._product_types}",
                )

            symbol_product_pairs.add((raw_symbol, product_type))

        all_pyo3_instruments = []

        # Request specific symbols to avoid pagination issues when loading many instruments
        for raw_symbol, product_type in symbol_product_pairs:
            pyo3_instruments = await self._client.request_instruments(
                product_type,
                raw_symbol,
            )
            all_pyo3_instruments.extend(pyo3_instruments)

        self._instruments_pyo3 = all_pyo3_instruments
        instruments = instruments_from_pyo3(all_pyo3_instruments)

        for instrument in instruments:
            if instrument.id not in instrument_ids:
                continue  # Filter instrument ID
            self.add(instrument=instrument)

    async def load_async(self, instrument_id: InstrumentId, filters: dict | None = None) -> None:
        PyCondition.not_none(instrument_id, "instrument_id")
        await self.load_ids_async([instrument_id], filters)
