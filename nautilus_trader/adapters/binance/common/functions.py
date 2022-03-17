# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import List

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType


def parse_symbol(symbol: str, account_type: BinanceAccountType):
    symbol = symbol.upper()
    if account_type.is_spot or account_type.is_margin:
        return symbol

    # Parse Futures symbol
    if symbol[-1].isdigit():
        return symbol  # Deliverable
    if symbol.endswith("_PERP"):
        symbol = symbol.replace("_", "-")
        return symbol
    else:
        return symbol + "-PERP"


def format_symbol(symbol: str):
    return symbol.upper().replace(" ", "").replace("/", "").replace("-PERP", "")


def convert_symbols_list_to_json_array(symbols: List[str]):
    if symbols is None:
        return symbols
    formatted_symbols: List[str] = [format_symbol(s) for s in symbols]
    return json.dumps(formatted_symbols).replace(" ", "").replace("/", "")
