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
"""
Load venue instruments for the template data client.
"""

from typing import override

from nautilus_trader.live.providers import InstrumentProvider
from nautilus_trader.model import Currency
from nautilus_trader.model import CurrencyPair
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol

from .constants import INSTRUMENT_ID


class TemplateInstrumentProvider(InstrumentProvider):
    """
    Load the deterministic currency pair into local storage.
    """

    @override
    async def load_all_async(self, filters: dict | None = None) -> None:
        """
        Load the template currency pair without network access.
        """
        instrument = CurrencyPair(
            instrument_id=INSTRUMENT_ID,
            raw_symbol=Symbol("EURUSD"),
            base_currency=Currency.from_str("EUR"),
            quote_currency=Currency.from_str("USD"),
            price_precision=5,
            size_precision=0,
            price_increment=Price.from_str("0.00001"),
            size_increment=Quantity.from_str("1"),
            ts_event=11,
            ts_init=13,
        )
        self.add(instrument)

    @override
    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        """
        Load requested instruments using the base provider implementation.
        """
        return await super().load_ids_async(instrument_ids, filters)

    @override
    async def load_async(self, instrument_id: InstrumentId, filters: dict | None = None) -> None:
        """
        Load requested instruments using the base provider implementation.
        """
        return await super().load_async(instrument_id, filters)
