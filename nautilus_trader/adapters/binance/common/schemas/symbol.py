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

import json

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType


################################################################################
# HTTP responses
################################################################################


class BinanceSymbol(str):
    """Binance compatible symbol"""

    def __new__(cls, symbol: str):
        # Format the string on construction to be binance compatible
        symbol.upper()
        symbol.replace(" ", "")
        symbol.replace("/", "")
        symbol.replace("-PERP", "")
        return super().__new__(cls, symbol)

    def parse_binance_to_internal(self, account_type: BinanceAccountType) -> str:
        if account_type.is_spot_or_margin:
            return self

        # Parse Futures symbol
        if self[-1].isdigit():
            return self  # Deliverable
        if self.endswith("_PERP"):
            return self.replace("_", "-")
        else:
            return self + "-PERP"


class BinanceSymbols(str):
    """Binance compatible list of symbols"""

    def __new__(cls, symbols: list[str]):
        binance_symbols: list[BinanceSymbol] = [BinanceSymbol(symbol) for symbol in symbols]
        return super().__new__(cls, json.dumps(binance_symbols))

    def parse_str_to_list(self) -> list[BinanceSymbol]:
        binance_symbols: list[BinanceSymbol] = json.loads(self)
        return binance_symbols
