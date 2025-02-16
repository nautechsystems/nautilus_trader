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

from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import ExecEngineConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import PortfolioConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.config import StreamingConfig
from nautilus_trader.core.data import Data
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


_AAPL_US = TestInstrumentProvider.equity(symbol="AAPL", venue="NASDAQ")


class TestConfigStubs:
    @staticmethod
    def streaming_config(
        catalog: ParquetDataCatalog,
    ) -> StreamingConfig:
        return StreamingConfig(
            catalog_path=str(catalog.path),
            fs_protocol=catalog.fs_protocol,
        )

    @staticmethod
    def backtest_venue_config() -> BacktestVenueConfig:
        return BacktestVenueConfig(
            name="NASDAQ",
            oms_type="NETTING",
            account_type="CASH",
            base_currency="USD",
            starting_balances=["10000 USD"],
            book_type="L2_MBP",
        )

    @staticmethod
    def order_book_imbalance(
        instrument_id: InstrumentId | None = None,
    ) -> ImportableStrategyConfig:
        return ImportableStrategyConfig(
            strategy_path="nautilus_trader.examples.strategies.orderbook_imbalance:OrderBookImbalance",
            config_path="nautilus_trader.examples.strategies.orderbook_imbalance:OrderBookImbalanceConfig",
            config={
                "instrument_id": instrument_id or _AAPL_US,
                "max_trade_size": 50,
            },
        )

    @staticmethod
    def exec_engine_config() -> ExecEngineConfig:
        return ExecEngineConfig(debug=True)

    @staticmethod
    def risk_engine_config() -> RiskEngineConfig:
        return RiskEngineConfig(
            bypass=True,
            max_order_submit_rate="100/00:00:01",
            max_order_modify_rate="100/00:00:01",
            max_notional_per_order={"AAPL": 100_000},
        )

    @staticmethod
    def portfolio_config() -> PortfolioConfig:
        return PortfolioConfig(debug=True)

    @staticmethod
    def strategies_config() -> list[ImportableStrategyConfig]:
        return [
            ImportableStrategyConfig(
                strategy_path="nautilus_trader.examples.strategies.orderbook_imbalance:OrderBookImbalance",
                config_path="nautilus_trader.examples.strategies.orderbook_imbalance:OrderBookImbalanceConfig",
                config={
                    "instrument_id": _AAPL_US.id,
                    "max_trade_size": 50,
                },
            ),
        ]

    @staticmethod
    def backtest_engine_config(
        catalog: ParquetDataCatalog,
        log_level="INFO",
        bypass_logging: bool = True,
        bypass_risk: bool = False,
        persist: bool = False,
        strategies: list[ImportableStrategyConfig] | None = None,
    ) -> BacktestEngineConfig:
        if persist:
            assert catalog is not None, "If `persist=True`, must pass `catalog`"
        return BacktestEngineConfig(
            logging=LoggingConfig(log_level=log_level, bypass_logging=bypass_logging),
            exec_engine=ExecEngineConfig(),
            risk_engine=RiskEngineConfig(bypass=bypass_risk),
            streaming=TestConfigStubs.streaming_config(catalog=catalog) if persist else None,
            strategies=strategies or [],
        )

    @staticmethod
    def venue_config() -> BacktestVenueConfig:
        return BacktestVenueConfig(
            name="SIM",
            oms_type="HEDGING",
            account_type="MARGIN",
            base_currency="USD",
            starting_balances=["1000000 USD"],
        )

    @staticmethod
    def backtest_data_config(
        catalog: ParquetDataCatalog,
        data_cls: Data = QuoteTick,
        instrument_id: str | None = None,
    ) -> BacktestDataConfig:
        return BacktestDataConfig(
            data_cls=data_cls.fully_qualified_name(),
            catalog_path=str(catalog.path),
            catalog_fs_protocol=catalog.fs_protocol,
            instrument_id=instrument_id,
        )

    @staticmethod
    def backtest_run_config(
        catalog: ParquetDataCatalog,
        config: BacktestEngineConfig | None = None,
        instrument_ids: list[str] | None = None,
        data_types: tuple[Data, ...] = (QuoteTick,),
        venues: list[BacktestVenueConfig] | None = None,
    ) -> BacktestRunConfig:
        instrument_ids = instrument_ids or [TestIdStubs.betting_instrument_id().value]
        run_config = BacktestRunConfig(
            engine=config,
            venues=venues or [TestConfigStubs.backtest_venue_config()],
            data=[
                TestConfigStubs.backtest_data_config(
                    catalog=catalog,
                    data_cls=cls,
                    instrument_id=instrument_id,
                )
                for cls in data_types
                for instrument_id in instrument_ids
            ],
            chunk_size=5_000,
        )
        return run_config
