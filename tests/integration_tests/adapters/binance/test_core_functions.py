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

import pytest

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.functions import convert_symbols_list_to_json_array
from nautilus_trader.adapters.binance.common.functions import format_symbol


class TestBinanceCoreFunctions:
    def test_format_symbol(self):
        # Arrange
        symbol = "ethusdt-perp"

        # Act
        result = format_symbol(symbol)

        # Assert
        assert result == "ETHUSDT"

    def test_convert_symbols_list_to_json_array(self):
        # Arrange
        symbols = ["BTCUSDT", "ETHUSDT-PERP", " XRDUSDT"]

        # Act
        result = convert_symbols_list_to_json_array(symbols)

        # Assert
        assert result == '["BTCUSDT","ETHUSDT","XRDUSDT"]'

    @pytest.mark.parametrize(
        "account_type, expected",
        [
            [BinanceAccountType.SPOT, True],
            [BinanceAccountType.MARGIN_CROSS, False],
            [BinanceAccountType.MARGIN_ISOLATED, False],
            [BinanceAccountType.FUTURES_USDT, False],
            [BinanceAccountType.FUTURES_COIN, False],
        ],
    )
    def test_binance_account_type_is_spot(self, account_type, expected):
        # Arrange, Act, Assert
        assert account_type.is_spot == expected

    @pytest.mark.parametrize(
        "account_type, expected",
        [
            [BinanceAccountType.SPOT, False],
            [BinanceAccountType.MARGIN_CROSS, True],
            [BinanceAccountType.MARGIN_ISOLATED, True],
            [BinanceAccountType.FUTURES_USDT, False],
            [BinanceAccountType.FUTURES_COIN, False],
        ],
    )
    def test_binance_account_type_is_margin(self, account_type, expected):
        # Arrange, Act, Assert
        assert account_type.is_margin == expected

    @pytest.mark.parametrize(
        "account_type, expected",
        [
            [BinanceAccountType.SPOT, False],
            [BinanceAccountType.MARGIN_CROSS, False],
            [BinanceAccountType.MARGIN_ISOLATED, False],
            [BinanceAccountType.FUTURES_USDT, True],
            [BinanceAccountType.FUTURES_COIN, True],
        ],
    )
    def test_binance_account_type_is_futures(self, account_type, expected):
        # Arrange, Act, Assert
        assert account_type.is_futures == expected
