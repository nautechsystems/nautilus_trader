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
import sys
from decimal import Decimal
from typing import List

import pytest

from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.backtest.config import BacktestRunConfig
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.backtest.results import BacktestResult
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.util import parse_bytes
from nautilus_trader.trading.config import ImportableStrategyConfig
from tests.test_kit.mocks.data import aud_usd_data_loader
from tests.test_kit.mocks.data import data_catalog_setup


pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="test path broken on windows")


@pytest.mark.skip(reason="unify system kernel")
class TestBacktestNode:
    def setup(self):
        data_catalog_setup()
        self.catalog = DataCatalog.from_env()
        self.venue_config = BacktestVenueConfig(
            name="SIM",
            oms_type="HEDGING",
            account_type="MARGIN",
            base_currency="USD",
            starting_balances=["1000000 USD"],
            # fill_model=fill_model,  # TODO(cs): Implement next iteration
        )
        self.data_config = BacktestDataConfig(
            catalog_path="/.nautilus/catalog",
            catalog_fs_protocol="memory",
            data_cls=QuoteTick,
            instrument_id="AUD/USD.SIM",
            start_time=1580398089820000000,
            end_time=1580504394501000000,
        )
        self.backtest_configs = [
            BacktestRunConfig(
                engine=BacktestEngineConfig(),
                venues=[self.venue_config],
                data=[self.data_config],
            )
        ]
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
        self.backtest_configs_strategies = [
            self.backtest_configs[0].replace(strategies=self.strategies)
        ]
        aud_usd_data_loader()  # Load sample data

    def test_init(self):
        node = BacktestNode()
        assert node

    def test_build_graph_shared_nodes(self):
        # Arrange
        node = BacktestNode()
        graph = node.build_graph(self.backtest_configs)
        dsk = graph.dask.to_dict()

        # Act - The strategies share the same input data,
        result = sorted([k.split("-")[0] for k in dsk.keys()])

        # Assert
        assert result == [
            "_gather_delayed",
            "_run_delayed",
        ]

    @pytest.mark.parametrize("batch_size_bytes", [None, parse_bytes("1mib")])
    def test_backtest_against_example_run(self, batch_size_bytes):
        """Replicate examples/fx_ema_cross_audusd_ticks.py backtest result."""
        # Arrange
        config = BacktestRunConfig(
            engine=BacktestEngineConfig(),
            venues=[self.venue_config],
            data=[self.data_config],
            strategies=self.strategies,
            batch_size_bytes=batch_size_bytes,
        )

        node = BacktestNode()

        # Act
        tasks = node.build_graph([config])
        results: List[BacktestResult] = tasks.compute()

        # Assert
        assert len(results) == 1  # TODO(cs): More asserts obviously
        # assert len(result.account_balances) == 193
        # assert len(result.positions) == 48
        # assert len(result.fill_report) == 96
        # account_result = result.account_balances.iloc[-2].to_dict()
        # expected = {
        #     "account_id": "SIM-001",
        #     "account_type": "MARGIN",
        #     "base_currency": "USD",
        #     "currency": "USD",
        #     "free": "994356.25",
        #     "info": b"{}",  # noqa: P103
        #     "locked": "2009.63",
        #     "reported": False,
        #     "total": "996365.88",
        #     "venue": Venue("SIM"),
        # }
        # assert account_result == expected

    def test_backtest_run_sync(self):
        # Arrange
        node = BacktestNode()

        # Act
        results = node.run_sync(run_configs=self.backtest_configs_strategies)

        # Assert
        assert len(results) == 1

    def test_backtest_run_streaming_sync(self):
        # Arrange
        node = BacktestNode()
        base = self.backtest_configs[0]
        config = base.replace(strategies=self.strategies, batch_size_bytes=parse_bytes("10kib"))

        # Act
        results = node.run_sync([config])

        # Assert
        assert len(results) == 1

    def test_backtest_build_graph(self):
        # Arrange
        node = BacktestNode()
        tasks = node.build_graph(self.backtest_configs_strategies)

        # Act
        result: List[BacktestResult] = tasks.compute()

        # Assert
        assert len(result) == 1

    # def test_backtest_run_distributed(self):
    #     from distributed import Client
    #
    #     # Arrange
    #     node = BacktestNode()
    #     with Client(processes=False):
    #         tasks = node.build_graph(self.backtest_configs_strategies)
    #
    #         # Act
    #         result = tasks.compute()
    #
    #         # Assert
    #         assert result

    def test_backtest_run_results(self):
        # Arrange
        node = BacktestNode()

        # Act
        results = node.run_sync(self.backtest_configs_strategies)

        # Assert
        assert isinstance(results, list)
        assert len(results) == 1
        # assert (  # TODO(cs): string changed
        #     str(results[0])
        #     == "BacktestResult(trader_id='BACKTESTER-000', machine_id='CJDS-X99-Ubuntu', run_config_id='e7647ae948f030bbd50e0b6cb58f67ae', instance_id='ecdf513e-9b07-47d5-9742-3b984a27bb52', run_id='d4d7a09c-fac7-4240-b80a-fd7a7d8f217c', run_started=1648796370520892000, run_finished=1648796371603767000, backtest_start=1580398089820000000, backtest_end=1580504394500999936, elapsed_time=106304.680999, iterations=100000, total_events=192, total_orders=96, total_positions=48, stats_pnls={'USD': {'PnL': -3634.12, 'PnL%': Decimal('-0.36341200'), 'Max Winner': 2673.19, 'Avg Winner': 530.0907692307693, 'Min Winner': 123.13, 'Min Loser': -16.86, 'Avg Loser': -263.9497142857143, 'Max Loser': -616.84, 'Expectancy': -48.89708333333337, 'Win Rate': 0.2708333333333333}}, stats_returns={'Annual Volatility (Returns)': 0.01191492048585753, 'Average (Return)': -3.3242292920660964e-05, 'Average Loss (Return)': -0.00036466955522398476, 'Average Win (Return)': 0.0007716524869588397, 'Sharpe Ratio': -0.7030729097982443, 'Sortino Ratio': -1.492072178035927, 'Profit Factor': 0.8713073377919724, 'Risk Return Ratio': -0.04428943030649289})"  # noqa
        # )

    def test_node_config_from_raw(self):
        # Arrange
        raw = json.dumps(
            {
                "engine": {
                    "trader_id": "Test-111",
                    "log_level": "INFO",
                },
                "venues": [
                    {
                        "name": "SIM",
                        "oms_type": "HEDGING",
                        "account_type": "MARGIN",
                        "base_currency": "USD",
                        "starting_balances": ["1000000 USD"],
                    }
                ],
                "data": [
                    {
                        "catalog_path": "/.nautilus/catalog",
                        "catalog_fs_protocol": "memory",
                        "data_cls": QuoteTick.fully_qualified_name(),
                        "instrument_id": "AUD/USD.SIM",
                        "start_time": 1580398089820000000,
                        "end_time": 1580504394501000000,
                    }
                ],
                "strategies": [
                    {
                        "strategy_path": "nautilus_trader.examples.strategies.ema_cross:EMACross",
                        "config_path": "nautilus_trader.examples.strategies.ema_cross:EMACrossConfig",
                        "config": {
                            "instrument_id": "AUD/USD.SIM",
                            "bar_type": "AUD/USD.SIM-100-TICK-MID-INTERNAL",
                            "fast_ema_period": 10,
                            "slow_ema_period": 20,
                            "trade_size": 1_000_000,
                            "order_id_tag": "001",
                        },
                    }
                ],
            }
        )

        # Act
        config = BacktestRunConfig.parse_raw(raw)
        node = BacktestNode()

        # Assert
        node.run_sync(run_configs=[config])
