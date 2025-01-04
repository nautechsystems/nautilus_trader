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

import pytest

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.common.symbol import BinanceSymbols


class TestBinanceCoreFunctions:
    def test_format_symbol(self):
        # Arrange
        symbol = "ethusdt-perp"

        # Act
        result = BinanceSymbol(symbol)

        # Assert
        assert result == "ETHUSDT"

    def test_convert_symbols_list_to_json_array(self):
        # Arrange
        symbols = ["BTCUSDT", "ETHUSDT-PERP", " XRDUSDT"]

        # Act
        result = BinanceSymbols(symbols)

        # Assert
        assert result == '["BTCUSDT","ETHUSDT","XRDUSDT"]'

    @pytest.mark.parametrize(
        ("account_type", "expected"),
        [
            [BinanceAccountType.SPOT, True],
            [BinanceAccountType.MARGIN, False],
            [BinanceAccountType.ISOLATED_MARGIN, False],
            [BinanceAccountType.USDT_FUTURE, False],
            [BinanceAccountType.COIN_FUTURE, False],
        ],
    )
    def test_binance_account_type_is_spot(self, account_type, expected):
        # Arrange, Act, Assert
        assert account_type.is_spot == expected

    @pytest.mark.parametrize(
        ("account_type", "expected"),
        [
            [BinanceAccountType.SPOT, False],
            [BinanceAccountType.MARGIN, True],
            [BinanceAccountType.ISOLATED_MARGIN, True],
            [BinanceAccountType.USDT_FUTURE, False],
            [BinanceAccountType.COIN_FUTURE, False],
        ],
    )
    def test_binance_account_type_is_margin(self, account_type, expected):
        # Arrange, Act, Assert
        assert account_type.is_margin == expected

    @pytest.mark.parametrize(
        ("account_type", "expected"),
        [
            [BinanceAccountType.SPOT, True],
            [BinanceAccountType.MARGIN, True],
            [BinanceAccountType.ISOLATED_MARGIN, True],
            [BinanceAccountType.USDT_FUTURE, False],
            [BinanceAccountType.COIN_FUTURE, False],
        ],
    )
    def test_binance_account_type_is_spot_or_margin(self, account_type, expected):
        # Arrange, Act, Assert
        assert account_type.is_spot_or_margin == expected

    @pytest.mark.parametrize(
        ("account_type", "expected"),
        [
            [BinanceAccountType.SPOT, False],
            [BinanceAccountType.MARGIN, False],
            [BinanceAccountType.ISOLATED_MARGIN, False],
            [BinanceAccountType.USDT_FUTURE, True],
            [BinanceAccountType.COIN_FUTURE, True],
        ],
    )
    def test_binance_account_type_is_futures(self, account_type, expected):
        # Arrange, Act, Assert
        assert account_type.is_futures == expected
