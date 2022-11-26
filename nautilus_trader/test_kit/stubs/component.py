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

import asyncio
from typing import Optional

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LogLevelParser
from nautilus_trader.core.data import Data
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.mocks.engines import MockLiveDataEngine
from nautilus_trader.test_kit.mocks.engines import MockLiveExecutionEngine
from nautilus_trader.test_kit.mocks.engines import MockLiveRiskEngine
from nautilus_trader.test_kit.stubs.config import TestConfigStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


class TestComponentStubs:
    @staticmethod
    def clock() -> LiveClock:
        return LiveClock()

    @staticmethod
    def logger(level="INFO") -> LiveLogger:
        return LiveLogger(
            loop=asyncio.get_event_loop(),
            clock=TestComponentStubs.clock(),
            level_stdout=LogLevelParser.from_str_py(level),
        )

    @staticmethod
    def msgbus():
        return MessageBus(
            trader_id=TestIdStubs.trader_id(),
            clock=TestComponentStubs.clock(),
            logger=TestComponentStubs.logger(),
        )

    @staticmethod
    def cache():
        return Cache(
            database=None,
            logger=TestComponentStubs.logger(),
        )

    @staticmethod
    def portfolio():
        return Portfolio(
            msgbus=TestComponentStubs.msgbus(),
            clock=TestComponentStubs.clock(),
            cache=TestComponentStubs.cache(),
            logger=TestComponentStubs.logger(),
        )

    @staticmethod
    def trading_strategy():
        strategy = Strategy()
        strategy.register(
            trader_id=TraderId("TESTER-000"),
            portfolio=TestComponentStubs.portfolio(),
            msgbus=TestComponentStubs.msgbus(),
            cache=TestComponentStubs.cache(),
            logger=TestComponentStubs.logger(),
            clock=TestComponentStubs.clock(),
        )
        return strategy

    @staticmethod
    def mock_live_data_engine():
        return MockLiveDataEngine(
            loop=asyncio.get_event_loop(),
            msgbus=TestComponentStubs.msgbus(),
            cache=TestComponentStubs.cache(),
            clock=TestComponentStubs.clock(),
            logger=TestComponentStubs.logger(),
        )

    @staticmethod
    def mock_live_exec_engine():
        return MockLiveExecutionEngine(
            loop=asyncio.get_event_loop(),
            msgbus=TestComponentStubs.msgbus(),
            cache=TestComponentStubs.cache(),
            clock=TestComponentStubs.clock(),
            logger=TestComponentStubs.logger(),
        )

    @staticmethod
    def mock_live_risk_engine():
        return MockLiveRiskEngine(
            loop=asyncio.get_event_loop(),
            portfolio=TestComponentStubs.portfolio(),
            msgbus=TestComponentStubs.msgbus(),
            cache=TestComponentStubs.cache(),
            clock=TestComponentStubs.clock(),
            logger=TestComponentStubs.logger(),
        )

    @staticmethod
    def order_factory():
        return OrderFactory(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            clock=TestComponentStubs.clock(),
        )

    @staticmethod
    def backtest_node(
        catalog: ParquetDataCatalog,
        engine_config: BacktestEngineConfig,
    ) -> BacktestNode:
        run_config = TestConfigStubs.backtest_run_config(catalog=catalog, config=engine_config)
        node = BacktestNode(configs=[run_config])
        return node

    @staticmethod
    def backtest_engine(
        config: Optional[BacktestEngineConfig] = None,
        instrument: Optional[Instrument] = None,
        ticks: list[Data] = None,
        venue: Optional[Venue] = None,
        oms_type: Optional[OMSType] = None,
        account_type: Optional[AccountType] = None,
        base_currency: Optional[Currency] = None,
        starting_balances: Optional[list[Money]] = None,
        fill_model: Optional[FillModel] = None,
    ) -> BacktestEngine:
        engine = BacktestEngine(config=config)
        engine.add_venue(
            venue=venue or Venue("SIM"),
            oms_type=oms_type or OMSType.HEDGING,
            account_type=account_type or AccountType.MARGIN,
            base_currency=base_currency or USD,
            starting_balances=starting_balances or [Money(1_000_000, USD)],
            fill_model=fill_model or FillModel(),
        )
        engine.add_instrument(instrument)

        if ticks:
            engine.add_data(ticks)

        return engine
