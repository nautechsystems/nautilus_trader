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
"""
Represent a dYdX specific symbol containing a product type suffix.
"""

from __future__ import annotations

from nautilus_trader.adapters.dydx.common.constants import DYDX_VENUE
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol


class DYDXSymbol(str):
    """
    Represent a dYdX specific symbol containing a product type suffix.
    """

    __slots__ = ()

    def __new__(cls, symbol: str) -> DYDXSymbol:  # noqa: PYI034
        """
        Create a new dYdX symbol.
        """
        PyCondition.valid_string(symbol, "symbol")

        # Format the string on construction to be dYdX compatible
        return super().__new__(
            cls,
            symbol.upper().replace(" ", "").replace("/", "").replace("-PERP", ""),
        )

    @property
    def raw_symbol(self) -> str:
        """
        Return the raw Bybit symbol (without the product type suffix).

        Returns
        -------
        str

        """
        return str(self)

    def to_instrument_id(self) -> InstrumentId:
        """
        Parse the dYdX symbol into a Nautilus instrument ID.

        Returns
        -------
        InstrumentId

        """
        return InstrumentId(Symbol(str(self) + "-PERP"), DYDX_VENUE)
