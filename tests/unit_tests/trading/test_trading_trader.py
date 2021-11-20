# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal

from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.backtest.data_client import BacktestMarketDataClient
from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.common.actor import Actor
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.config import ActorConfig
from nautilus_trader.common.logging import Logger
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ComponentId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.trading.strategy import TradingStrategy
from nautilus_trader.trading.strategy import TradingStrategyConfig
from nautilus_trader.trading.trader import Trader
from tests.test_kit.stubs import TestStubs


USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestTrader:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.logger = Logger(self.clock)

        self.trader_id = TestStubs.trader_id()
        self.account_id = TestStubs.account_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestStubs.cache()

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

        self.data_engine.process(USDJPY_SIM)

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exchange = SimulatedExchange(
            venue=Venue("SIM"),
            venue_type=VenueType.ECN,
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            default_leverage=Decimal(50),
            leverages={},
            is_frozen_account=False,
            cache=self.cache,
            instruments=[USDJPY_SIM],
            modules=[],
            fill_model=FillModel(),
            clock=self.clock,
            logger=self.logger,
        )

        self.data_client = BacktestMarketDataClient(
            client_id=ClientId("SIM"),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            account_id=self.account_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Wire up components
        self.data_engine.register_client(self.data_client)
        self.exec_engine.register_client(self.exec_client)

        self.trader = Trader(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            portfolio=self.portfolio,
            data_engine=self.data_engine,
            risk_engine=self.risk_engine,
            exec_engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
        )

    def test_initialize_trader(self):
        # Arrange, Act, Assert
        assert self.trader.id == TraderId("TESTER-000")
        assert self.trader.is_initialized
        assert len(self.trader.strategy_states()) == 0

    def test_add_strategy(self):
        # Arrange, Act
        self.trader.add_strategy(TradingStrategy())

        # Assert
        assert self.trader.strategy_states() == {StrategyId("TradingStrategy-000"): "INITIALIZED"}

    def test_add_strategies(self):
        # Arrange
        strategies = [
            TradingStrategy(TradingStrategyConfig(order_id_tag="001")),
            TradingStrategy(TradingStrategyConfig(order_id_tag="002")),
        ]

        # Act
        self.trader.add_strategies(strategies)

        # Assert
        assert self.trader.strategy_states() == {
            StrategyId("TradingStrategy-001"): "INITIALIZED",
            StrategyId("TradingStrategy-002"): "INITIALIZED",
        }

    def test_clear_strategies(self):
        # Arrange
        strategies = [
            TradingStrategy(TradingStrategyConfig(order_id_tag="001")),
            TradingStrategy(TradingStrategyConfig(order_id_tag="002")),
        ]
        self.trader.add_strategies(strategies)

        # Act
        self.trader.clear_strategies()

        # Assert
        assert self.trader.strategy_states() == {}

    def test_add_actor(self):
        # Arrange
        config = ActorConfig(component_id="MyPlugin-01")
        actor = Actor(config)

        # Act
        self.trader.add_actor(actor)

        # Assert
        assert self.trader.actor_ids() == [ComponentId("MyPlugin-01")]

    def test_add_actors(self):
        # Arrange
        actors = [
            Actor(ActorConfig(component_id="MyPlugin-01")),
            Actor(ActorConfig(component_id="MyPlugin-02")),
        ]

        # Act
        self.trader.add_actors(actors)

        # Assert
        assert self.trader.actor_ids() == [
            ComponentId("MyPlugin-01"),
            ComponentId("MyPlugin-02"),
        ]

    def test_clear_actors(self):
        # Arrange
        actors = [
            Actor(ActorConfig(component_id="MyPlugin-01")),
            Actor(ActorConfig(component_id="MyPlugin-02")),
        ]
        self.trader.add_actors(actors)

        # Act
        self.trader.clear_actors()

        # Assert
        assert self.trader.actor_ids() == []

    def test_get_strategy_states(self):
        # Arrange
        strategies = [
            TradingStrategy(TradingStrategyConfig(order_id_tag="001")),
            TradingStrategy(TradingStrategyConfig(order_id_tag="002")),
        ]
        self.trader.add_strategies(strategies)

        # Act
        status = self.trader.strategy_states()

        # Assert
        assert StrategyId("TradingStrategy-001") in status
        assert StrategyId("TradingStrategy-002") in status
        assert status[StrategyId("TradingStrategy-001")] == "INITIALIZED"
        assert status[StrategyId("TradingStrategy-002")] == "INITIALIZED"
        assert len(status) == 2

    def test_change_strategies(self):
        # Arrange
        strategies = [
            TradingStrategy(TradingStrategyConfig(order_id_tag="003")),
            TradingStrategy(TradingStrategyConfig(order_id_tag="004")),
        ]

        # Act
        self.trader.add_strategies(strategies)

        # Assert
        assert strategies[0].id in self.trader.strategy_states()
        assert strategies[1].id in self.trader.strategy_states()
        assert len(self.trader.strategy_states()) == 2

    def test_start_a_trader(self):
        # Arrange
        strategies = [
            TradingStrategy(TradingStrategyConfig(order_id_tag="001")),
            TradingStrategy(TradingStrategyConfig(order_id_tag="002")),
        ]
        self.trader.add_strategies(strategies)

        # Act
        self.trader.start()

        strategy_states = self.trader.strategy_states()

        # Assert
        assert self.trader.is_running
        assert strategy_states[StrategyId("TradingStrategy-001")] == "RUNNING"
        assert strategy_states[StrategyId("TradingStrategy-002")] == "RUNNING"

    def test_stop_a_running_trader(self):
        # Arrange
        strategies = [
            TradingStrategy(TradingStrategyConfig(order_id_tag="001")),
            TradingStrategy(TradingStrategyConfig(order_id_tag="002")),
        ]
        self.trader.add_strategies(strategies)
        self.trader.start()

        # Act
        self.trader.stop()

        strategy_states = self.trader.strategy_states()

        # Assert
        assert self.trader.is_stopped
        assert strategy_states[StrategyId("TradingStrategy-001")] == "STOPPED"
        assert strategy_states[StrategyId("TradingStrategy-002")] == "STOPPED"

    def test_subscribe_to_msgbus_topic_adds_subscription(self):
        # Arrange
        consumer = []

        # Act
        self.trader.subscribe("events*", consumer.append)

        # Assert
        assert len(self.msgbus.subscriptions("events*")) == 6
        assert "events*" in self.msgbus.topics()
        assert self.msgbus.subscriptions("events*")[-1].handler == consumer.append

    def test_unsubscribe_from_msgbus_topic_removes_subscription(self):
        # Arrange
        consumer = []
        self.trader.subscribe("events*", consumer.append)

        # Act
        self.trader.unsubscribe("events*", consumer.append)

        # Assert
        assert len(self.msgbus.subscriptions("events*")) == 5
