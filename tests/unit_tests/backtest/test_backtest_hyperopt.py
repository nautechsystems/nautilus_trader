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

from decimal import Decimal

import hyperopt
import pytest

from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.hyperopt import HyperoptBacktestNode
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.model.data.tick import QuoteTick
from tests.test_kit.mocks.data import aud_usd_data_loader
from tests.test_kit.mocks.data import data_catalog_setup


class TestHyperoptBacktestNode:
    def setup(self):
        self.catalog = data_catalog_setup()
        self.venue_config = BacktestVenueConfig(
            name="SIM",
            oms_type="HEDGING",
            account_type="MARGIN",
            base_currency="USD",
            starting_balances=["1000000 USD"],
        )
        self.data_config = BacktestDataConfig(
            catalog_path="/.nautilus/catalog",
            catalog_fs_protocol="memory",
            data_cls=QuoteTick,
            instrument_id="AUD/USD.SIM",
            start_time=1580398089820000000,
            end_time=1580504394501000000,
        )
        self.strategies = [
            ImportableStrategyConfig(
                strategy_path="nautilus_trader.examples.strategies.ema_cross:EMACross",
                config_path="nautilus_trader.examples.strategies.ema_cross:EMACrossConfig",
                config=dict(
                    instrument_id="AUD/USD.SIM",
                    bar_type="AUD/USD.SIM-100-TICK-MID-INTERNAL",
                    fast_ema_period=10,
                    slow_ema_period=20,
                    trade_size=Decimal(1_000_000),
                    order_id_tag="001",
                ),
            )
        ]
        self.base_config = BacktestRunConfig(
            engine=BacktestEngineConfig(strategies=self.strategies),
            venues=[self.venue_config],
            data=[self.data_config],
        )
        aud_usd_data_loader()  # Load sample data

    def test_init(self):
        # Arrange
        node = HyperoptBacktestNode(base_config=self.base_config)

        # Act
        node.set_strategy_config(
            strategy_path="nautilus_trader.examples.strategies.ema_cross:EMACross",
            config_path="nautilus_trader.examples.strategies.ema_cross:EMACrossConfig",
        )

        # Assert (requires more investigation to make a better test)
        with pytest.raises(hyperopt.exceptions.AllTrialsFailed):
            node.hyperopt_search(
                params=dict(
                    instrument_id="AUD/USD.SIM",
                    bar_type="AUD/USD.SIM-100-TICK-MID-INTERNAL",
                    fast_ema_period=10,
                    slow_ema_period=20,
                    trade_size=Decimal(1_000_000),
                    order_id_tag="001",
                ),
                max_evals=2,
            )
