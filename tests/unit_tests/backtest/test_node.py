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
import pytest

from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.common.config import InvalidConfiguration
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.identifiers import InstrumentId
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
            # fill_model=fill_model,  # TODO: Implement
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
                chunk_size=5_000,
            ),
        ]
        load_catalog_with_stub_quote_ticks_audusd(self.catalog)  # Load sample data

    def test_init(self):
        # Arrange, Act
        node = BacktestNode(configs=self.backtest_configs)

        # Assert
        assert node

    @pytest.mark.parametrize(
        ("book_type"),
        [
            "L2_MBP",
            "L3_MBO",
        ],
    )
    def test_order_book_with_depth_data_config_validation(self, book_type: str) -> None:
        # Arrange
        venue_l3 = BacktestVenueConfig(
            name="SIM",
            oms_type="HEDGING",
            account_type="MARGIN",
            base_currency="USD",
            book_type=book_type,
            starting_balances=["1_000_000 USD"],
        )

        run_config = BacktestRunConfig(
            engine=BacktestEngineConfig(
                strategies=self.strategies,
                logging=LoggingConfig(bypass_logging=True),
            ),
            venues=[self.venue_config, venue_l3],
            data=[self.data_config],
            chunk_size=None,  # No streaming
        )

        with pytest.raises(InvalidConfiguration) as exc_info:
            BacktestNode(configs=[run_config])

        assert (
            str(exc_info.value)
            == f"No order book data available for SIM with book type {book_type}"
        )

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
            chunk_size=5_000,
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
