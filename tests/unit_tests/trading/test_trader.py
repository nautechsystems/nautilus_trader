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

from decimal import Decimal
from typing import Any

import pytest

from nautilus_trader.backtest.data_client import BacktestMarketDataClient
from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import MakerTakerFeeModel
from nautilus_trader.common import Environment
from nautilus_trader.common.actor import Actor
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.config import ActorConfig
from nautilus_trader.config import ExecAlgorithmConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.examples.strategies.blank import MyStrategy
from nautilus_trader.examples.strategies.blank import MyStrategyConfig
from nautilus_trader.execution.algorithm import ExecAlgorithm
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ComponentId
from nautilus_trader.model.identifiers import ExecAlgorithmId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy
from nautilus_trader.trading.trader import Trader


USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestTrader:
    def setup(self) -> None:
        # Fixture Setup
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.data_engine.process(USDJPY_SIM)

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exchange = SimulatedExchange(
            venue=Venue("SIM"),
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            default_leverage=Decimal(50),
            leverages={},
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            modules=[],
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            clock=self.clock,
        )
        self.exchange.add_instrument(USDJPY_SIM)

        self.data_client = BacktestMarketDataClient(
            client_id=ClientId("SIM"),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Wire up components
        self.data_engine.register_client(self.data_client)
        self.exec_engine.register_client(self.exec_client)

        self.trader = Trader(
            trader_id=self.trader_id,
            instance_id=UUID4(),
            msgbus=self.msgbus,
            cache=self.cache,
            portfolio=self.portfolio,
            data_engine=self.data_engine,
            risk_engine=self.risk_engine,
            exec_engine=self.exec_engine,
            clock=self.clock,
            environment=Environment.BACKTEST,
        )

    def test_initialize_trader(self) -> None:
        # Arrange, Act, Assert
        assert self.trader.id == TraderId("TESTER-000")
        assert self.trader.is_initialized
        assert len(self.trader.strategy_states()) == 0

    def test_add_strategy(self) -> None:
        # Arrange, Act
        self.trader.add_strategy(Strategy())

        # Assert
        assert self.trader.strategy_states() == {StrategyId("Strategy-000"): "READY"}

    def test_start_actor_when_not_exists(self) -> None:
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            self.trader.start_actor(ComponentId("UNKNOWN-000"))

    def test_start_actor(self) -> None:
        # Arrange
        actor = Actor()
        self.trader.add_actor(actor)

        # Act
        self.trader.start_actor(actor.id)

        # Assert
        assert actor.is_running
        assert self.trader.actor_states() == {actor.id: "RUNNING"}

    def test_start_actor_when_already_started(self) -> None:
        # Arrange
        actor = Actor()
        self.trader.add_actor(actor)
        self.trader.start_actor(actor.id)

        # Act
        self.trader.start_actor(actor.id)

        # Assert
        assert actor.is_running
        assert self.trader.actor_states() == {actor.id: "RUNNING"}

    def test_stop_actor(self) -> None:
        # Arrange
        actor = Actor()
        self.trader.add_actor(actor)
        self.trader.start_actor(actor.id)

        # Act
        self.trader.stop_actor(actor.id)

        # Assert
        assert not actor.is_running
        assert self.trader.actor_states() == {actor.id: "STOPPED"}

    def test_stop_actor_when_already_stopped(self) -> None:
        # Arrange
        actor = Actor()
        self.trader.add_actor(actor)
        self.trader.start_actor(actor.id)
        self.trader.stop_actor(actor.id)

        # Act
        self.trader.stop_actor(actor.id)

        # Assert
        assert not actor.is_running
        assert self.trader.actor_states() == {actor.id: "STOPPED"}

    def test_remove_actor(self) -> None:
        # Arrange
        actor = Actor()
        self.trader.add_actor(actor)
        self.trader.start_actor(actor.id)

        # Act
        self.trader.remove_actor(actor.id)

        # Assert
        assert not actor.is_running
        assert self.trader.actors() == []

    def test_start_strategy_when_not_exists(self) -> None:
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            self.trader.start_strategy(StrategyId("UNKNOWN-000"))

    def test_start_strategy(self) -> None:
        # Arrange
        strategy = Strategy()
        self.trader.add_strategy(strategy)

        # Act
        self.trader.start_strategy(strategy.id)

        # Assert
        assert strategy.is_running
        assert self.trader.strategy_states() == {strategy.id: "RUNNING"}

    def test_start_strategy_when_already_started(self) -> None:
        # Arrange
        strategy = Strategy()
        self.trader.add_strategy(strategy)
        self.trader.start_strategy(strategy.id)

        # Act
        self.trader.start_strategy(strategy.id)

        # Assert
        assert strategy.is_running
        assert self.trader.strategy_states() == {strategy.id: "RUNNING"}

    def test_stop(self) -> None:
        # Arrange
        strategy = Strategy()
        self.trader.add_strategy(strategy)
        self.trader.start_strategy(strategy.id)

        # Act
        self.trader.stop_strategy(strategy.id)

        # Assert
        assert not strategy.is_running
        assert self.trader.strategy_states() == {strategy.id: "STOPPED"}

    def test_stop_strategy_when_already_stopped(self) -> None:
        # Arrange
        strategy = Strategy()
        self.trader.add_strategy(strategy)
        self.trader.start_strategy(strategy.id)
        self.trader.stop_strategy(strategy.id)

        # Act
        self.trader.stop_strategy(strategy.id)

        # Assert
        assert not strategy.is_running
        assert self.trader.strategy_states() == {strategy.id: "STOPPED"}

    def test_remove_strategy(self) -> None:
        # Arrange
        strategy = Strategy()
        self.trader.add_strategy(strategy)
        self.trader.start_strategy(strategy.id)

        # Act
        self.trader.remove_strategy(strategy.id)

        # Assert
        assert not strategy.is_running
        assert self.trader.strategies() == []

    def test_add_strategies_with_no_order_id_tags(self) -> None:
        # Arrange
        strategies = [Strategy(), Strategy()]

        # Act
        self.trader.add_strategies(strategies)

        # Assert
        assert self.trader.strategy_states() == {
            StrategyId("Strategy-000"): "READY",
            StrategyId("Strategy-001"): "READY",
        }

    def test_add_strategies_with_duplicate_order_id_tags_raises_runtime_error(self) -> None:
        # Arrange
        config = MyStrategyConfig(
            instrument_id=USDJPY_SIM.id,
            order_id_tag="000",  # <-- will be a duplicate
        )
        strategies = [Strategy(), MyStrategy(config=config)]

        # Act, Assert
        with pytest.raises(RuntimeError):
            self.trader.add_strategies(strategies)

    def test_add_strategies(self) -> None:
        # Arrange
        strategies = [
            Strategy(StrategyConfig(order_id_tag="001")),
            Strategy(StrategyConfig(order_id_tag="002")),
        ]

        # Act
        self.trader.add_strategies(strategies)

        # Assert
        assert self.trader.strategy_states() == {
            StrategyId("Strategy-001"): "READY",
            StrategyId("Strategy-002"): "READY",
        }

    def test_clear_strategies(self) -> None:
        # Arrange
        strategies = [
            Strategy(StrategyConfig(order_id_tag="001")),
            Strategy(StrategyConfig(order_id_tag="002")),
        ]
        self.trader.add_strategies(strategies)

        # Act
        self.trader.clear_strategies()

        # Assert
        assert self.trader.strategy_states() == {}

    def test_add_actor(self) -> None:
        # Arrange
        config = ActorConfig(component_id="MyPlugin-01")
        actor = Actor(config)

        # Act
        self.trader.add_actor(actor)

        # Assert
        assert self.trader.actor_ids() == [ComponentId("MyPlugin-01")]

    def test_add_actors(self) -> None:
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

    def test_clear_actors(self) -> None:
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

    def test_get_strategy_states(self) -> None:
        # Arrange
        strategies = [
            Strategy(StrategyConfig(order_id_tag="001")),
            Strategy(StrategyConfig(order_id_tag="002")),
        ]
        self.trader.add_strategies(strategies)

        # Act
        status = self.trader.strategy_states()

        # Assert
        assert StrategyId("Strategy-001") in status
        assert StrategyId("Strategy-002") in status
        assert status[StrategyId("Strategy-001")] == "READY"
        assert status[StrategyId("Strategy-002")] == "READY"
        assert len(status) == 2

    def test_add_exec_algorithm(self) -> None:
        # Arrange
        exec_algorithm = ExecAlgorithm()

        # Act
        self.trader.add_exec_algorithm(exec_algorithm)

        # Assert
        assert self.trader.exec_algorithm_ids() == [exec_algorithm.id]
        assert self.trader.exec_algorithms() == [exec_algorithm]
        assert self.trader.exec_algorithm_states() == {exec_algorithm.id: "READY"}

    def test_change_exec_algorithms(self) -> None:
        # Arrange
        exec_algorithm1 = ExecAlgorithm(
            ExecAlgorithmConfig(exec_algorithm_id=ExecAlgorithmId("001")),
        )
        exec_algorithm2 = ExecAlgorithm(
            ExecAlgorithmConfig(exec_algorithm_id=ExecAlgorithmId("002")),
        )
        exec_algorithms = [exec_algorithm1, exec_algorithm2]

        # Act
        self.trader.add_exec_algorithms(exec_algorithms)

        # Assert
        assert self.trader.exec_algorithm_ids() == [exec_algorithm1.id, exec_algorithm2.id]
        assert self.trader.exec_algorithms() == [exec_algorithm1, exec_algorithm2]
        assert self.trader.exec_algorithm_states() == {
            exec_algorithm1.id: "READY",
            exec_algorithm2.id: "READY",
        }

    def test_clear_exec_algorithms(self) -> None:
        # Arrange
        exec_algorithms = [
            ExecAlgorithm(ExecAlgorithmConfig(exec_algorithm_id=ExecAlgorithmId("001"))),
            ExecAlgorithm(ExecAlgorithmConfig(exec_algorithm_id=ExecAlgorithmId("002"))),
        ]

        self.trader.add_exec_algorithms(exec_algorithms)

        # Act
        self.trader.clear_exec_algorithms()

        # Assert
        assert self.trader.exec_algorithm_ids() == []
        assert self.trader.exec_algorithms() == []
        assert self.trader.exec_algorithm_states() == {}

    def test_change_strategies(self) -> None:
        # Arrange
        strategy1 = Strategy(StrategyConfig(order_id_tag="003"))
        strategy2 = Strategy(StrategyConfig(order_id_tag="004"))

        strategies = [strategy1, strategy2]

        # Act
        self.trader.add_strategies(strategies)

        # Assert
        assert strategy1.id in self.trader.strategy_states()
        assert strategy2.id in self.trader.strategy_states()
        assert len(self.trader.strategy_states()) == 2

    def test_start_a_trader(self) -> None:
        # Arrange
        strategies = [
            Strategy(StrategyConfig(order_id_tag="001")),
            Strategy(StrategyConfig(order_id_tag="002")),
        ]
        self.trader.add_strategies(strategies)

        # Act
        self.trader.start()

        strategy_states = self.trader.strategy_states()

        # Assert
        assert self.trader.is_running
        assert strategy_states[StrategyId("Strategy-001")] == "RUNNING"
        assert strategy_states[StrategyId("Strategy-002")] == "RUNNING"

    def test_stop_a_running_trader(self) -> None:
        # Arrange
        strategies = [
            Strategy(StrategyConfig(order_id_tag="001")),
            Strategy(StrategyConfig(order_id_tag="002")),
        ]
        self.trader.add_strategies(strategies)
        self.trader.start()

        # Act
        self.trader.stop()

        strategy_states = self.trader.strategy_states()

        # Assert
        assert self.trader.is_stopped
        assert strategy_states[StrategyId("Strategy-001")] == "STOPPED"
        assert strategy_states[StrategyId("Strategy-002")] == "STOPPED"

    def test_subscribe_to_msgbus_topic_adds_subscription(self) -> None:
        # Arrange
        consumer: list[Any] = []

        # Act
        self.trader.subscribe("events*", consumer.append)

        # Assert
        assert len(self.msgbus.subscriptions("events*")) == 5
        assert "events*" in self.msgbus.topics()
        assert self.msgbus.subscriptions("events*")[-1].handler == consumer.append

    def test_unsubscribe_from_msgbus_topic_removes_subscription(self) -> None:
        # Arrange
        consumer: list[Any] = []
        self.trader.subscribe("events*", consumer.append)

        # Act
        self.trader.unsubscribe("events*", consumer.append)

        # Assert
        assert len(self.msgbus.subscriptions("events*")) == 4
