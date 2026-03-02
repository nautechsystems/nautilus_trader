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

from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import KrakenProductType
from nautilus_trader.model.instruments import instruments_from_pyo3


class KrakenInstrumentProvider(InstrumentProvider):
    """
    Provides Nautilus instrument definitions from Kraken.

    Parameters
    ----------
    http_client_spot : nautilus_pyo3.KrakenSpotHttpClient, optional
        The Kraken Spot HTTP client.
    http_client_futures : nautilus_pyo3.KrakenFuturesHttpClient, optional
        The Kraken Futures HTTP client.
    product_types : list[KrakenProductType], optional
        The Kraken product types to load.
        If ``None`` then defaults to [KrakenProductType.SPOT].
    config : InstrumentProviderConfig, optional
        The instrument provider configuration, by default None.

    """

    def __init__(
        self,
        http_client_spot: nautilus_pyo3.KrakenSpotHttpClient | None = None,
        http_client_futures: nautilus_pyo3.KrakenFuturesHttpClient | None = None,
        product_types: list[KrakenProductType] | None = None,
        config: InstrumentProviderConfig | None = None,
    ) -> None:
        super().__init__(config=config)
        self._http_client_spot = http_client_spot
        self._http_client_futures = http_client_futures
        self._product_types = product_types or [KrakenProductType.SPOT]
        self._log_warnings = config.log_warnings if config else True

        self._instruments_pyo3: list[nautilus_pyo3.Instrument] = []

    @property
    def product_types(self) -> list[KrakenProductType]:
        """
        Return the product types configured for this provider.

        Returns
        -------
        list[KrakenProductType]

        """
        return self._product_types.copy()

    def instruments_pyo3(self) -> list[Any]:
        """
        Return all Kraken PyO3 instrument definitions held by the provider.

        Returns
        -------
        list[nautilus_pyo3.Instrument]

        """
        return self._instruments_pyo3

    async def load_all_async(self, filters: dict | None = None) -> None:
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        all_pyo3_instruments: list[nautilus_pyo3.Instrument] = []

        for product_type in self._product_types:
            if product_type == KrakenProductType.SPOT:
                if self._http_client_spot is None:
                    self._log.warning("No HTTP client configured for Spot")
                    continue
                self._log.info("Loading Spot instruments...")
                pyo3_instruments = await self._http_client_spot.request_instruments()
                all_pyo3_instruments.extend(pyo3_instruments)
                self._log.info(f"Loaded {len(pyo3_instruments)} Spot instruments")
            elif product_type == KrakenProductType.FUTURES:
                if self._http_client_futures is None:
                    self._log.warning("No HTTP client configured for Futures")
                    continue
                self._log.info("Loading Futures instruments...")
                pyo3_instruments = await self._http_client_futures.request_instruments()
                all_pyo3_instruments.extend(pyo3_instruments)
                self._log.info(f"Loaded {len(pyo3_instruments)} Futures instruments")

        self._instruments_pyo3 = all_pyo3_instruments
        instruments = instruments_from_pyo3(all_pyo3_instruments)
        for instrument in instruments:
            self.add(instrument=instrument)
