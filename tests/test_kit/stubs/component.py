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

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LogLevelParser
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.mocks import MockLiveDataEngine
from tests.test_kit.mocks import MockLiveExecutionEngine
from tests.test_kit.mocks import MockLiveRiskEngine
from tests.test_kit.stubs.identities import TestIdStubs


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
        strategy = TradingStrategy()
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
