import sys
from decimal import Decimal
from typing import List

import pytest
from dask.utils import parse_bytes

from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.backtest.config import BacktestRunConfig
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.backtest.results import BacktestResult
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.trading.config import ImportableStrategyConfig
from tests.test_kit.mocks import aud_usd_data_loader
from tests.test_kit.mocks import data_catalog_setup


pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="test path broken on windows")


class TestBacktestNode:
    def setup(self):
        data_catalog_setup()
        self.catalog = DataCatalog.from_env()
        self.venue_config = BacktestVenueConfig(
            name="SIM",
            venue_type="ECN",
            oms_type="HEDGING",
            account_type="MARGIN",
            base_currency="USD",
            starting_balances=["1000000 USD"],
            # fill_model=fill_model,  # TODO(cs): Implement next iteration
        )
        self.data_config = BacktestDataConfig(
            catalog_path="/root",
            catalog_fs_protocol="memory",
            data_cls_path="nautilus_trader.model.data.tick.QuoteTick",
            instrument_id="AUD/USD.SIM",
            start_time=1580398089820000000,
            end_time=1580504394501000000,
        )
        self.backtest_configs = [
            BacktestRunConfig(
                engine=BacktestEngineConfig(bypass_logging=True),
                venues=[self.venue_config],
                data=[self.data_config],
            )
        ]
        self.strategies = [
            ImportableStrategyConfig(
                path="nautilus_trader.examples.strategies.ema_cross:EMACross",
                config=EMACrossConfig(
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

    @pytest.mark.skip(reason="fix on develop")
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

    @pytest.mark.skip(reason="fix on develop")
    def test_backtest_build_graph(self):
        # Arrange
        node = BacktestNode()
        tasks = node.build_graph(self.backtest_configs_strategies)

        # Act
        result: List[BacktestResult] = tasks.compute()

        # Assert
        assert len(result.results) == 1

    @pytest.mark.skip(reason="fix on develop")
    def test_backtest_run_distributed(self):
        from distributed import Client

        # Arrange
        node = BacktestNode()
        with Client(processes=False):
            tasks = node.build_graph(self.backtest_configs_strategies)

            # Act
            result = tasks.compute()

            # Assert
            assert result

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
        #     == "BacktestResult(backtest-2432fd8e1f2bb4b85ce7383712a66edf, SIM[USD]=996365.88)"
        # )

    def test_backtest_run_custom_summary(self):
        # Arrange
        def buy_count(engine):
            return {"buys": len([o for o in engine.cache.orders() if o.side == OrderSide.BUY])}

        backtest_configs_strategies = self.backtest_configs_strategies[0].replace(
            engine=BacktestEngineConfig(custom_summaries=(("buy_count", buy_count),))
        )
        node = BacktestNode()

        # Act
        results = node.run_sync([backtest_configs_strategies])

        # Assert
        assert results[0].custom_summaries["buy_count"] == {"buys": 48}
