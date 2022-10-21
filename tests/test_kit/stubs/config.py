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

from typing import Optional

from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import ExecEngineConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.config import StreamingConfig
from nautilus_trader.core.data import Data
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog


AAPL_US = TestInstrumentProvider.aapl_equity()


class TestConfigStubs:
    @staticmethod
    def streaming_config(
        catalog: ParquetDataCatalog,
        kind: str = "backtest",
    ) -> StreamingConfig:
        return StreamingConfig(
            catalog_path=str(catalog.path),
            fs_protocol=catalog.fs_protocol,
            kind=kind,
        )

    @staticmethod
    def backtest_venue_config() -> BacktestVenueConfig:
        return BacktestVenueConfig(  # type: ignore
            name="NASDAQ",
            oms_type="NETTING",
            account_type="CASH",
            base_currency="USD",
            starting_balances=["10000 USD"],
            book_type="L2_MBP",
        )

    @staticmethod
    def order_book_imbalance(instrument_id=None) -> ImportableStrategyConfig:
        return ImportableStrategyConfig(
            strategy_path="nautilus_trader.examples.strategies.orderbook_imbalance:OrderBookImbalance",
            config_path="nautilus_trader.examples.strategies.orderbook_imbalance:OrderBookImbalanceConfig",
            config=dict(
                instrument_id=instrument_id or AAPL_US,
                max_trade_size=50,
            ),
        )

    @staticmethod
    def backtest_engine_config(
        log_level="INFO",
        bypass_logging=True,
        bypass_risk=False,
        allow_cash_position=True,
        persist=False,
        catalog: Optional[ParquetDataCatalog] = None,
        strategies: list[ImportableStrategyConfig] = None,
    ) -> BacktestEngineConfig:
        if persist:
            assert catalog is not None, "If `persist=True`, must pass `catalog`"
        return BacktestEngineConfig(
            log_level=log_level,
            bypass_logging=bypass_logging,
            exec_engine=ExecEngineConfig(allow_cash_positions=allow_cash_position),
            risk_engine=RiskEngineConfig(bypass=bypass_risk),
            streaming=TestConfigStubs.streaming_config(catalog=catalog) if persist else None,
            strategies=strategies or [],
        )

    @staticmethod
    def backtest_run_config(
        catalog: ParquetDataCatalog,
        config: Optional[BacktestEngineConfig] = None,
        instrument_ids: Optional[list[str]] = None,
        data_types: tuple[Data] = (QuoteTick,),
        venues: Optional[list[Venue]] = None,
    ):
        instrument_ids = instrument_ids or []
        run_config = BacktestRunConfig(  # type: ignore
            engine=config,
            venues=venues or [TestConfigStubs.backtest_venue_config()],
            data=[
                BacktestDataConfig(  # type: ignore
                    data_cls=cls.fully_qualified_name(),
                    catalog_path=str(catalog.path),
                    catalog_fs_protocol=catalog.fs_protocol,
                    instrument_id=instrument_id,
                )
                for cls in data_types
                for instrument_id in instrument_ids
            ],
        )
        return run_config
