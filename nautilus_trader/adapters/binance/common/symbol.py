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

from __future__ import annotations

import json

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.core.correctness import PyCondition


################################################################################
# HTTP responses
################################################################################


class BinanceSymbol(str):
    """
    Binance compatible symbol.
    """

    def __new__(cls, symbol: str) -> BinanceSymbol:  # noqa: PYI034
        PyCondition.valid_string(symbol, "symbol")

        # Format the string on construction to be Binance compatible
        formatted = symbol.upper().replace(" ", "").replace("/", "")

        # Convert Nautilus format back to Binance format for perpetual symbols.
        # COIN-M (inverse): BTCUSD-PERP → BTCUSD_PERP (Binance uses underscore)
        # USDT-M (linear):  BTCUSDT-PERP → BTCUSDT (Binance has no suffix)
        if formatted.endswith("-PERP"):
            base = formatted[:-5]
            # Detect Linear (stablecoin-quoted) symbols.
            # Must avoid false positives where COIN-M {BASE}USD overlaps with
            # stablecoin names: e.g. BNBUSD matches "BUSD", DOTUSD matches "TUSD".
            # Fix: after removing the stablecoin suffix, require ≥3 chars remaining
            # (all Binance crypto tickers are ≥3 chars). This ensures BNBUSD is
            # treated as BNB+USD (COIN-M) not BN+BUSD (Linear).
            _LINEAR_QUOTES = ("USDT", "BUSD", "TUSD", "FDUSD", "USDC", "DAI")
            is_linear = False
            for quote in _LINEAR_QUOTES:
                if base.endswith(quote) and len(base) > len(quote) + 2:
                    is_linear = True
                    break
            if is_linear:
                formatted = base
            else:
                formatted = base + "_PERP"

        return super().__new__(cls, formatted)

    def parse_as_nautilus(self, account_type: BinanceAccountType) -> str:
        if account_type.is_spot_or_margin:
            return str(self)

        # Parse Futures symbol
        if self[-1].isdigit():
            return str(self)  # Deliverable
        if self.endswith("_PERP"):
            return str(self).replace("_", "-")
        else:
            return str(self) + "-PERP"


class BinanceSymbols(str):
    """
    Binance compatible list of symbols.
    """

    def __new__(cls, symbols: list[str]) -> BinanceSymbols:  # noqa: PYI034
        PyCondition.not_empty(symbols, "symbols")

        binance_symbols: list[BinanceSymbol] = [BinanceSymbol(symbol) for symbol in symbols]
        return super().__new__(cls, json.dumps(binance_symbols).replace(" ", ""))

    def parse_str_to_list(self) -> list[BinanceSymbol]:
        binance_symbols: list[BinanceSymbol] = json.loads(self)
        return binance_symbols
