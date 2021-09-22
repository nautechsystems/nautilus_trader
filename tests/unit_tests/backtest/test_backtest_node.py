from decimal import Decimal

from dask.utils import parse_bytes

from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.backtest.config import BacktestRunConfig
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.backtest.results import BacktestResult
from nautilus_trader.backtest.results import BacktestRunResults
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.trading.config import ImportableStrategyConfig
from tests.test_kit.mocks import aud_usd_data_loader
from tests.test_kit.mocks import data_catalog_setup
from tests.test_kit.strategies import EMACrossConfig


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
            data_type=QuoteTick,
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
                path="tests.test_kit.strategies:EMACross",
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
            "_run_delayed",
            "load",
        ]

    def test_backtest_against_example(self):
        """Replicate examples/fx_ema_cross_audusd_ticks.py backtest result."""
        # Arrange
        config = BacktestRunConfig(
            engine=BacktestEngineConfig(),
            venues=[self.venue_config],
            data=[self.data_config],
            strategies=self.strategies,
        )

        node = BacktestNode()

        # Act
        tasks = node.build_graph([config])
        results: BacktestRunResults = tasks.compute()
        result: BacktestResult = results.results[0]

        # Assert
        assert len(result.account_balances) == 193
        assert len(result.positions) == 48
        assert len(result.fill_report) == 96
        account_result = result.account_balances.iloc[-2].to_dict()
        expected = {
            "account_id": "SIM-001",
            "account_type": "MARGIN",
            "base_currency": "USD",
            "currency": "USD",
            "free": "976269.59",
            "info": b"{}",  # noqa: P103
            "locked": "20096.29",
            "reported": False,
            "total": "996365.88",
            "venue": Venue("SIM"),
        }
        assert account_result == expected

    def test_backtest_run_sync(self):
        # Arrange
        node = BacktestNode()

        # Act
        config = self.backtest_configs[0].replace(strategies=self.strategies)
        result = node.run_sync([config])

        # Assert
        assert len(result.results) == 1

    def test_backtest_run_streaming_sync(self):
        # Arrange
        node = BacktestNode()
        base = self.backtest_configs[0]
        config = base.replace(strategies=self.strategies, batch_size_bytes=parse_bytes("1mib"))

        # Act
        result = node.run_sync([config])

        # Assert
        assert len(result.results) == 1

    def test_backtest_build_graph(self):
        # Arrange
        node = BacktestNode()
        tasks = node.build_graph(self.backtest_configs)

        # Act
        result: BacktestRunResults = tasks.compute()

        # Assert
        assert len(result.results) == 2

    def test_backtest_run_distributed(self):
        from distributed import Client

        # Arrange
        node = BacktestNode()
        with Client(processes=False):
            tasks = node.build_graph(self.backtest_configs)

            # Act
            result = tasks.compute()

            # Assert
            assert result

    def test_backtest_run_results(self):
        # Arrange
        node = BacktestNode()

        # Act
        result = node.run_sync(self.backtest_configs)

        # Assert
        assert isinstance(result, BacktestRunResults)
        assert len(result.results) == 2
        assert (
            str(result.results[0])
            == "BacktestResult(backtest-c2c5a31261ee3c438a03f8bc3a7746f5, SIM[USD]=1000000.00)"
        )
