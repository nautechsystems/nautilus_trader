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

import pkgutil

import orjson
import pytest

from nautilus_trader.examples.strategies.ema_cross import EMACross
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig
from nautilus_trader.trading.config import ImportableStrategyConfig
from nautilus_trader.trading.config import StrategyFactory


class TestStrategyFactory:
    @pytest.mark.skip(reason="WIP")
    def test_create_from_source(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id="AUD/USD.SIM",
            bar_type="AUD/USD.SIM-1000-TICK-MID-INTERNAL",
            trade_size=1_000_000,
        )

        source = pkgutil.get_data("nautilus_trader.examples.strategies", "ema_cross.py")
        importable = ImportableStrategyConfig(
            module="my_ema_cross",
            source=source,
            config=config,
        )

        # Act
        strategy = StrategyFactory.create(importable)

        # Assert
        assert isinstance(strategy, EMACross)
        assert (
            repr(config)
            == "EMACrossConfig(order_id_tag='000', oms_type='HEDGING', instrument_id='AUD/USD.SIM', bar_type='AUD/USD.SIM-1000-TICK-MID-INTERNAL', fast_ema_period=10, slow_ema_period=20, trade_size=Decimal('1000000'))"  # noqa
        )

    def test_create_from_path(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id="AUD/USD.SIM",
            bar_type="AUD/USD.SIM-15-MINUTE-BID-EXTERNAL",
            trade_size=1_000_000,
            fast_ema_period=10,
            slow_ema_period=20,
        )
        importable = ImportableStrategyConfig(
            path="nautilus_trader.examples.strategies.ema_cross:EMACross",
            config=config,
        )

        # Act
        strategy = StrategyFactory.create(importable)

        # Assert
        assert isinstance(strategy, EMACross)
        assert (
            repr(config) == "EMACrossConfig(component_id=None, order_id_tag='000', oms_type=None, "
            "instrument_id='AUD/USD.SIM', bar_type='AUD/USD.SIM-15-MINUTE-BID-EXTERNAL', "
            "fast_ema_period=10, slow_ema_period=20, trade_size=Decimal('1000000'))"  # noqa
        )

    def test_create_from_raw(self):
        # Arrange
        raw = orjson.dumps(
            {
                "path": "nautilus_trader.examples.strategies.volatility_market_maker:VolatilityMarketMakerConfig",
                "config": {
                    "instrument_id": "ETHUSDT-PERP.BINANCE",
                    "bar_type": "ETHUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL",
                    "atr_period": "20",
                    "atr_multiple": "6.0",
                    "trade_size": "0.01",
                },
            }
        )

        # Act
        config = ImportableStrategyConfig.parse_raw(raw)

        # Assert
        assert config.cls == "VolatilityMarketMakerConfig"
        assert config.config.instrument_id == "ETHUSDT-PERP.BINANCE"
        assert config.config.atr_period == 20
