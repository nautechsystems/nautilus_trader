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

from decimal import Decimal

import msgspec.json

from nautilus_trader.config import ImportableConfig
from nautilus_trader.config import StrategyConfig


class TestStrategyConfig(StrategyConfig, kw_only=True, frozen=True):
    instrument_id: str
    trade_size: Decimal
    period: int
    close_positions_on_stop: bool = True


class TestConfigCommon:
    def test_importable_config_simple(self):
        # Arrange
        raw = msgspec.json.encode(
            {
                "path": "nautilus_trader.adapters.binance.config:BinanceDataClientConfig",
                "config": {
                    "api_key": "abc",
                },
            },
        )

        # Act
        config = msgspec.json.decode(raw, type=ImportableConfig).create()

        # Assert
        assert config.api_key == "abc"

    def test_return_false_for_invalid_data_types_1(self):
        # Arrange
        config = TestStrategyConfig(
            instrument_id=123,  # <-- instrument_id is not str
            trade_size=Decimal("100_0000"),
            period=20,
            close_positions_on_stop=True,
        )

        # Assert
        assert config.validate() is False

    def test_return_false_for_invalid_data_types_2(self):
        # Arrange
        config = TestStrategyConfig(
            instrument_id="EUR/USD.IDEALPRO",
            trade_size="100_0000",  # <-- trade_size is not Decimal
            period=20,
            close_positions_on_stop=True,
        )

        # Assert
        assert config.validate() is False

    def test_return_false_for_invalid_data_types_3(self):
        # Arrange
        config = TestStrategyConfig(
            instrument_id="EUR/USD.IDEALPRO",
            trade_size=Decimal("100_0000"),
            period=20.0,  # <-- period is not int
            close_positions_on_stop=True,
        )

        # Assert
        assert config.validate() is False
