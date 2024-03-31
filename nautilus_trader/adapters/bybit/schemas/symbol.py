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

from nautilus_trader.adapters.bybit.common.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol


class BybitSymbol(str):
    def __new__(cls, symbol: str | None):
        if symbol is not None:
            # Check if it contains one dot BTCUSDT-LINEAR for example is the correct
            # bybit symbol format
            if (
                symbol.find("-SPOT") == -1
                and symbol.find("-LINEAR") == -1
                and symbol.find("-OPTION") == -1
            ):
                raise ValueError(
                    f"Invalid symbol {symbol}. Does not contain -LINEAR, -SPOT or -OPTION suffix",
                )
            return super().__new__(
                cls,
                symbol.upper(),
            )

    @property
    def raw_symbol(self) -> str:
        return str(self).split("-")[0]

    @property
    def product_type(self) -> BybitProductType:
        if "-LINEAR" in self:
            return BybitProductType.LINEAR
        elif "-SPOT" in self:
            return BybitProductType.SPOT
        elif "-OPTION" in self:
            return BybitProductType.OPTION
        else:
            raise ValueError(f"Unknown product type for symbol {self}")

    @property
    def is_spot(self) -> bool:
        return self.product_type == BybitProductType.SPOT

    @property
    def is_linear(self) -> bool:
        return self.product_type == BybitProductType.LINEAR

    @property
    def is_option(self) -> bool:
        return self.product_type == BybitProductType.OPTION

    def parse_as_nautilus(self) -> InstrumentId:
        instrument = InstrumentId(Symbol(str(self)), BYBIT_VENUE)
        return instrument
