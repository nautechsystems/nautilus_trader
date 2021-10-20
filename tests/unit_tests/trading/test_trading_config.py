# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.trading.config import ImportableStrategyConfig
from nautilus_trader.trading.config import StrategyFactory
from tests.test_kit.strategies import EMACross
from tests.test_kit.strategies import EMACrossConfig


class TestStrategyFactory:
    @pytest.mark.skip(reason="WIP")
    def test_create_from_source(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id="AUD/USD.SIM",
            bar_type="AUD/USD.SIM-1000-TICK-MID-INTERNAL",
            trade_size=1_000_000,
        )

        source = pkgutil.get_data("tests.test_kit", "strategies.py")
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
            == "EMACrossConfig(order_id_tag='000', oms_type='HEDGING', instrument_id='AUD/USD.SIM', bar_type='AUD/USD.SIM-1000-TICK-MID-INTERNAL', trade_size=Decimal('1000000'), fast_ema_period=10, slow_ema_period=20)"  # noqa
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
            path="tests.test_kit.strategies:EMACross",
            config=config,
        )

        # Act
        strategy = StrategyFactory.create(importable)

        # Assert
        assert isinstance(strategy, EMACross)
        assert (
            repr(config)
            == "EMACrossConfig(component_id=None, order_id_tag='000', oms_type='HEDGING', "
            "instrument_id='AUD/USD.SIM', bar_type='AUD/USD.SIM-15-MINUTE-BID-EXTERNAL', "
            "trade_size=Decimal('1000000'), fast_ema_period=10, slow_ema_period=20)"  # noqa
        )
