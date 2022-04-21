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

from typing import List, Optional, Tuple

from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import ExecEngineConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import PersistenceConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.core.data import Data
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.persistence.catalog import DataCatalog


AAPL_US = TestInstrumentProvider.aapl_equity()


class TestConfigStubs:
    @staticmethod
    def persistence_config(
        catalog: DataCatalog,
        kind: str = "backtest",
        persist_logs=False,
    ) -> PersistenceConfig:
        return PersistenceConfig(
            catalog_path=str(catalog.path),
            fs_protocol=catalog.fs_protocol,
            kind=kind,
            persit_logs=persist_logs,
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
    def backtest_run_config(
        catalog: DataCatalog,
        instrument_ids: Optional[List[str]] = None,
        data_types: Tuple[Data] = (QuoteTick,),
        venues: Optional[List[Venue]] = None,
        persist: bool = False,
        bypass_risk: bool = True,
        add_strategy: bool = True,
    ):
        instrument_ids = instrument_ids or []
        engine_config = BacktestEngineConfig(
            log_level="INFO",
            bypass_logging=True,
            exec_engine=ExecEngineConfig(allow_cash_positions=True),
            risk_engine=RiskEngineConfig(bypass=bypass_risk),
            persistence=TestConfigStubs.persistence_config(catalog=catalog) if persist else None,
            strategies=[
                TestConfigStubs.order_book_imbalance(instrument_id=instrument_id)
                for instrument_id in instrument_ids
            ]
            if add_strategy
            else None,
        )
        run_config = BacktestRunConfig(  # type: ignore
            engine=engine_config,
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
