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

import msgspec

from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import StrategyFactory
from nautilus_trader.examples.strategies.ema_cross import EMACross


class TestStrategyFactory:
    def test_create_from_path(self):
        # Arrange
        config = {
            "instrument_id": "AUD/USD.SIM",
            "bar_type": "AUD/USD.SIM-15-MINUTE-BID-EXTERNAL",
            "trade_size": 1_000_000,
            "fast_ema_period": 10,
            "slow_ema_period": 20,
        }
        importable = ImportableStrategyConfig(
            strategy_path="nautilus_trader.examples.strategies.ema_cross:EMACross",
            config_path="nautilus_trader.examples.strategies.ema_cross:EMACrossConfig",
            config=config,
        )

        # Act
        strategy = StrategyFactory.create(importable)

        # Assert
        assert isinstance(strategy, EMACross)
        assert (
            repr(config)
            == "{'instrument_id': 'AUD/USD.SIM', 'bar_type': 'AUD/USD.SIM-15-MINUTE-BID-EXTERNAL',"
            " 'trade_size': 1000000, 'fast_ema_period': 10, 'slow_ema_period': 20}"
        )

    def test_create_from_raw(self):
        # Arrange
        raw = msgspec.json.encode(
            {
                "strategy_path": "nautilus_trader.examples.strategies.volatility_market_maker:VolatilityMarketMaker",
                "config_path": "nautilus_trader.examples.strategies.volatility_market_maker:VolatilityMarketMakerConfig",
                "config": {
                    "instrument_id": "ETHUSDT-PERP.BINANCE",
                    "bar_type": "ETHUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL",
                    "atr_period": "20",
                    "atr_multiple": "6.0",
                    "trade_size": "0.01",
                },
            },
        )

        # Act
        config = ImportableStrategyConfig.parse(raw)

        # Assert
        assert isinstance(config, ImportableStrategyConfig)
        assert config.config["instrument_id"] == "ETHUSDT-PERP.BINANCE"
        assert config.config["atr_period"] == "20"
