# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.persistence.funcs import parse_bytes
from nautilus_trader.test_kit.mocks.data import load_catalog_with_stub_quote_ticks_audusd
from nautilus_trader.test_kit.mocks.data import setup_catalog


class TestBacktestNode:
    def setup(self):
        self.catalog = setup_catalog(protocol="file", path="./catalog")
        self.venue_config = BacktestVenueConfig(
            name="SIM",
            oms_type="HEDGING",
            account_type="MARGIN",
            base_currency="USD",
            starting_balances=["1000000 USD"],
            # fill_model=fill_model,  # TODO(cs): Implement next iteration
        )
        self.data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol=self.catalog.fs_protocol,
            data_cls=QuoteTick,
            instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
            start_time=1580398089820000000,
            end_time=1580504394501000000,
        )
        self.strategies = [
            ImportableStrategyConfig(
                strategy_path="nautilus_trader.examples.strategies.ema_cross:EMACross",
                config_path="nautilus_trader.examples.strategies.ema_cross:EMACrossConfig",
                config={
                    "instrument_id": "AUD/USD.SIM",
                    "bar_type": "AUD/USD.SIM-100-TICK-MID-INTERNAL",
                    "fast_ema_period": 10,
                    "slow_ema_period": 20,
                    "trade_size": "1_000_000",
                    "order_id_tag": "001",
                },
            ),
        ]
        self.backtest_configs = [
            BacktestRunConfig(
                engine=BacktestEngineConfig(
                    strategies=self.strategies,
                    logging=LoggingConfig(bypass_logging=True),
                ),
                venues=[self.venue_config],
                data=[self.data_config],
            ),
        ]
        load_catalog_with_stub_quote_ticks_audusd(self.catalog)  # Load sample data

    def test_init(self):
        node = BacktestNode(configs=self.backtest_configs)
        assert node

    def test_run(self):
        # Arrange
        node = BacktestNode(configs=self.backtest_configs)

        # Act
        results = node.run()

        # Assert
        assert len(results) == 1

    def test_backtest_run_batch_sync(self):
        # Arrange
        config = BacktestRunConfig(
            engine=BacktestEngineConfig(strategies=self.strategies),
            venues=[self.venue_config],
            data=[self.data_config],
            batch_size_bytes=parse_bytes("10kib"),
        )

        node = BacktestNode(configs=[config])

        # Act
        results = node.run()

        # Assert
        assert len(results) == 1

    def test_backtest_run_results(self):
        # Arrange
        node = BacktestNode(configs=self.backtest_configs)

        # Act
        results = node.run()

        # Assert
        assert isinstance(results, list)
        assert len(results) == 1
        # assert (
        #     str(results[0])
        #     == "BacktestResult(trader_id='BACKTESTER-000', machine_id='CJDS-X99-Ubuntu', run_config_id='e7647ae948f030bbd50e0b6cb58f67ae', instance_id='ecdf513e-9b07-47d5-9742-3b984a27bb52', run_id='d4d7a09c-fac7-4240-b80a-fd7a7d8f217c', run_started=1648796370520892000, run_finished=1648796371603767000, backtest_start=1580398089820000000, backtest_end=1580504394500999936, elapsed_time=106304.680999, iterations=100000, total_events=192, total_orders=96, total_positions=48, stats_pnls={'USD': {'PnL': -3634.12, 'PnL%': Decimal('-0.36341200'), 'Max Winner': 2673.19, 'Avg Winner': 530.0907692307693, 'Min Winner': 123.13, 'Min Loser': -16.86, 'Avg Loser': -263.9497142857143, 'Max Loser': -616.84, 'Expectancy': -48.89708333333337, 'Win Rate': 0.2708333333333333}}, stats_returns={'Annual Volatility (Returns)': 0.01191492048585753, 'Average (Return)': -3.3242292920660964e-05, 'Average Loss (Return)': -0.00036466955522398476, 'Average Win (Return)': 0.0007716524869588397, 'Sharpe Ratio': -0.7030729097982443, 'Sortino Ratio': -1.492072178035927, 'Profit Factor': 0.8713073377919724, 'Risk Return Ratio': -0.04428943030649289})"  # noqa
        # )

    def test_node_config_from_raw(self):
        # Arrange
        raw = msgspec.json.encode(
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
                    },
                ],
                "data": [
                    {
                        "catalog_path": "catalog",
                        "data_cls": "nautilus_trader.model.data:QuoteTick",
                        "instrument_id": "AUD/USD.SIM",
                        "start_time": 1580398089820000000,
                        "end_time": 1580504394501000000,
                    },
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
                    },
                ],
            },
        )

        # Act
        config = BacktestRunConfig.parse(raw)
        node = BacktestNode(configs=[config])

        # Assert
        node.run()
