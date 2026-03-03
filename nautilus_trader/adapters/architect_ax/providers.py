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

from decimal import Decimal
from typing import Any

from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.instruments import instruments_from_pyo3


class AxInstrumentProvider(InstrumentProvider):
    """
    Provides Nautilus instrument definitions from AX Exchange.

    Parameters
    ----------
    client : nautilus_pyo3.AxHttpClient
        The AX Exchange HTTP client.
    config : InstrumentProviderConfig, optional
        The instrument provider configuration, by default None.
    maker_fee : Decimal, optional
        The maker fee to apply to loaded instruments.
    taker_fee : Decimal, optional
        The taker fee to apply to loaded instruments.

    """

    def __init__(
        self,
        client: nautilus_pyo3.AxHttpClient,
        config: InstrumentProviderConfig | None = None,
        maker_fee: Decimal | None = None,
        taker_fee: Decimal | None = None,
    ) -> None:
        super().__init__(config=config)
        self._client = client
        self._maker_fee = maker_fee
        self._taker_fee = taker_fee
        self._log_warnings = config.log_warnings if config else True
        self._instruments_pyo3: list[Any] = []

    def instruments_pyo3(self) -> list[Any]:
        """
        Return all AX Exchange PyO3 instrument definitions held by the provider.

        Returns
        -------
        list[nautilus_pyo3.Instrument]

        """
        return self._instruments_pyo3

    async def load_all_async(self, filters: dict | None = None) -> None:
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        pyo3_instruments = await self._client.request_instruments(
            maker_fee=self._maker_fee,
            taker_fee=self._taker_fee,
        )
        self._instruments_pyo3 = pyo3_instruments

        instruments = instruments_from_pyo3(pyo3_instruments)
        for instrument in instruments:
            self.add(instrument)

        self._log.info(f"Loaded {len(instruments)} instruments")
