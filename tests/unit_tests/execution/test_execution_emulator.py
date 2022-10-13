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

import pytest

from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.config import ExecEngineConfig
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.emulator import OrderEmulator
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Quantity
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.trading.strategy import Strategy
from tests.test_kit.mocks.cache_database import MockCacheDatabase
from tests.test_kit.stubs.identifiers import TestIdStubs


ETHUSD_FTX = TestInstrumentProvider.ethusd_ftx()


class TestOrderEmulator:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.logger = Logger(
            clock=TestClock(),
            level_stdout=LogLevel.DEBUG,
        )

        self.trader_id = TestIdStubs.trader_id()
        self.strategy_id = TestIdStubs.strategy_id()
        self.account_id = TestIdStubs.account_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache_db = MockCacheDatabase(
            logger=self.logger,
        )

        self.cache = Cache(
            database=self.cache_db,
            logger=self.logger,
        )
        self.cache.add_instrument(ETHUSD_FTX)

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=ExecEngineConfig(debug=True),
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.emulator = OrderEmulator(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.strategy = Strategy()
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

    def test_subscribed_quotes_when_nothing_subscribed_returns_empty_list(self):
        # Arrange, Act
        subscriptions = self.emulator.subscribed_quotes

        # Assert
        assert subscriptions == []

    def test_subscribed_trades_when_nothing_subscribed_returns_empty_list(self):
        # Arrange, Act
        subscriptions = self.emulator.subscribed_trades

        # Assert
        assert subscriptions == []

    def test_get_commands_when_no_emulations_returns_empty_dict(self):
        # Arrange, Act
        commands = self.emulator.get_commands()

        # Assert
        assert commands == {}

    def test_get_matching_core_when_no_emulations_returns_none(self):
        # Arrange, Act
        matching_core = self.emulator.get_matching_core(ETHUSD_FTX.id)

        # Assert
        assert matching_core is None

    @pytest.mark.parametrize(
        "emulation_trigger",
        [
            TriggerType.DEFAULT,
            TriggerType.BID_ASK,
        ],
    )
    def test_submit_limit_order_with_emulation_trigger_default_and_bid_ask(
        self,
        emulation_trigger,
    ):
        # Arrange
        order = self.strategy.order_factory.limit(
            instrument_id=ETHUSD_FTX.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=ETHUSD_FTX.make_price(2000),
        )

        # Act
        self.strategy.submit_order(
            order=order,
            emulation_trigger=emulation_trigger,
            execution_algorithm=None,
            execution_params=None,
        )

        matching_core = self.emulator.get_matching_core(ETHUSD_FTX.id)

        # Assert
        assert matching_core is not None
        assert order in matching_core.get_orders()
        assert len(self.emulator.get_commands()) == 1
        assert self.emulator.subscribed_quotes == [InstrumentId.from_str("ETH/USD.FTX")]

    def test_submit_limit_order_with_emulation_trigger_last(self):
        # Arrange
        order = self.strategy.order_factory.limit(
            instrument_id=ETHUSD_FTX.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=ETHUSD_FTX.make_price(2000),
        )

        # Act
        self.strategy.submit_order(
            order=order,
            emulation_trigger=TriggerType.LAST,
            execution_algorithm=None,
            execution_params=None,
        )

        matching_core = self.emulator.get_matching_core(ETHUSD_FTX.id)

        # Assert
        assert matching_core is not None
        assert order in matching_core.get_orders()
        assert len(self.emulator.get_commands()) == 1
        assert self.emulator.subscribed_trades == [InstrumentId.from_str("ETH/USD.FTX")]
