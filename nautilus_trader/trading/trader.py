# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
"""
The `Trader` class is intended to manage a fleet of trading strategies within a running
instance of the platform.

A running instance could be either a test/backtest or live implementation - the
`Trader` will operate in the same way.

"""

from __future__ import annotations

import asyncio
from typing import Any, Callable

import pandas as pd

from nautilus_trader.analysis.reporter import ReportProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.actor import Actor
from nautilus_trader.common.clock import Clock
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.component import Component
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.algorithm import ExecAlgorithm
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.identifiers import ComponentId
from nautilus_trader.model.identifiers import ExecAlgorithmId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.trading.strategy import Strategy


class Trader(Component):
    """
    Provides a trader for managing a fleet of actors, execution algorithms and trading
    strategies.

    Parameters
    ----------
    trader_id : TraderId
        The ID for the trader.
    msgbus : MessageBus
        The message bus for the trader.
    cache : Cache
        The cache for the trader.
    portfolio : Portfolio
        The portfolio for the trader.
    data_engine : DataEngine
        The data engine for the trader.
    risk_engine : RiskEngine
        The risk engine for the trader.
    exec_engine : ExecutionEngine
        The execution engine for the trader.
    clock : Clock
        The clock for the trader.
    logger : Logger
        The logger for the trader.
    loop : asyncio.AbstractEventLoop, optional
        The event loop for the trader.
    config : dict[str, Any]
        The configuration for the trader.

    Raises
    ------
    ValueError
        If `portfolio` is not equal to the `exec_engine` portfolio.
    ValueError
        If `strategies` is ``None``.
    ValueError
        If `strategies` is empty.
    TypeError
        If `strategies` contains a type other than `Strategy`.

    """

    def __init__(
        self,
        trader_id: TraderId,
        msgbus: MessageBus,
        cache: Cache,
        portfolio: Portfolio,
        data_engine: DataEngine,
        risk_engine: RiskEngine,
        exec_engine: ExecutionEngine,
        clock: Clock,
        logger: Logger,
        loop: asyncio.AbstractEventLoop | None = None,
        config: dict | None = None,
    ) -> None:
        if config is None:
            config = {}
        super().__init__(
            clock=clock,
            logger=logger,
            component_id=trader_id,
            msgbus=msgbus,
            config=config,
        )

        self._loop = loop
        self._cache = cache
        self._portfolio = portfolio
        self._data_engine = data_engine
        self._risk_engine = risk_engine
        self._exec_engine = exec_engine

        self._actors: dict[ComponentId, Actor] = {}
        self._strategies: dict[StrategyId, Strategy] = {}
        self._exec_algorithms: dict[ExecAlgorithmId, ExecAlgorithm] = {}
        self._has_controller: bool = config.get("has_controller", False)

    def actors(self) -> list[Actor]:
        """
        Return the actors loaded in the trader.

        Returns
        -------
        list[Actor]

        """
        return list(self._actors.values())

    def strategies(self) -> list[Strategy]:
        """
        Return the strategies loaded in the trader.

        Returns
        -------
        list[Strategy]

        """
        return list(self._strategies.values())

    def exec_algorithms(self) -> list[ExecAlgorithm]:
        """
        Return the execution algorithms loaded in the trader.

        Returns
        -------
        list[ExecAlgorithms]

        """
        return list(self._exec_algorithms.values())

    def actor_ids(self) -> list[ComponentId]:
        """
        Return the actor IDs loaded in the trader.

        Returns
        -------
        list[ComponentId]

        """
        return sorted(self._actors.keys())

    def strategy_ids(self) -> list[StrategyId]:
        """
        Return the strategy IDs loaded in the trader.

        Returns
        -------
        list[StrategyId]

        """
        return sorted(self._strategies.keys())

    def exec_algorithm_ids(self) -> list[ExecAlgorithmId]:
        """
        Return the execution algorithm IDs loaded in the trader.

        Returns
        -------
        list[ExecAlgorithmId]

        """
        return sorted(self._exec_algorithms.keys())

    def actor_states(self) -> dict[ComponentId, str]:
        """
        Return the traders actor states.

        Returns
        -------
        dict[ComponentId, str]

        """
        return {k: v.state.name for k, v in self._actors.items()}

    def strategy_states(self) -> dict[StrategyId, str]:
        """
        Return the traders strategy states.

        Returns
        -------
        dict[StrategyId, str]

        """
        return {k: v.state.name for k, v in self._strategies.items()}

    def exec_algorithm_states(self) -> dict[ExecAlgorithmId, str]:
        """
        Return the traders execution algorithm states.

        Returns
        -------
        dict[ExecAlgorithmId, str]

        """
        return {k: v.state.name for k, v in self._exec_algorithms.items()}

    # -- ACTION IMPLEMENTATIONS -----------------------------------------------------------------------

    def _start(self) -> None:
        for actor in self._actors.values():
            actor.start()

        for strategy in self._strategies.values():
            strategy.start()

        for exec_algorithm in self._exec_algorithms.values():
            exec_algorithm.start()

    def _stop(self) -> None:
        for actor in self._actors.values():
            if actor.is_running:
                actor.stop()
            else:
                self._log.warning(f"{actor} already stopped.")

        for strategy in self._strategies.values():
            if strategy.is_running:
                strategy.stop()
            else:
                self._log.warning(f"{strategy} already stopped.")

        for exec_algorithm in self._exec_algorithms.values():
            if exec_algorithm.is_running:
                exec_algorithm.stop()
            else:
                self._log.warning(f"{exec_algorithm} already stopped.")

    def _reset(self) -> None:
        for actor in self._actors.values():
            actor.reset()

        for strategy in self._strategies.values():
            strategy.reset()

        for exec_algorithm in self._exec_algorithms.values():
            exec_algorithm.reset()

        self._portfolio.reset()

    def _dispose(self) -> None:
        for actor in self._actors.values():
            actor.dispose()

        for strategy in self._strategies.values():
            strategy.dispose()

        for exec_algorithm in self._exec_algorithms.values():
            exec_algorithm.dispose()

    # --------------------------------------------------------------------------------------------------

    def add_actor(self, actor: Actor) -> None:
        """
        Add the given custom component to the trader.

        Parameters
        ----------
        actor : Actor
            The actor to add and register.

        Raises
        ------
        ValueError
            If `actor.state` is ``RUNNING`` or ``DISPOSED``.
        RuntimeError
            If `actor.id` already exists in the trader.

        """
        PyCondition.true(not actor.is_running, "actor.state was RUNNING")
        PyCondition.true(not actor.is_disposed, "actor.state was DISPOSED")

        if self.is_running:
            self._log.error("Cannot add component to a running trader.")
            return

        if actor.id in self._actors:
            raise RuntimeError(
                f"Already registered an actor with ID {actor.id}, "
                "try specifying a different actor ID.",
            )

        if isinstance(self._clock, LiveClock):
            clock = self._clock.__class__(loop=self._loop)
        else:
            clock = self._clock.__class__()

        # Wire component into trader
        actor.register_base(
            msgbus=self._msgbus,
            cache=self._cache,
            clock=clock,  # Clock per component
            logger=self._log.get_logger(),
        )

        self._actors[actor.id] = actor

        self._log.info(f"Registered Component {actor}.")

    def add_actors(self, actors: list[Actor]) -> None:
        """
        Add the given actors to the trader.

        Parameters
        ----------
        actors : list[TradingStrategies]
            The actors to add and register.

        Raises
        ------
        ValueError
            If `actors` is ``None`` or empty.

        """
        PyCondition.not_empty(actors, "actors")

        for actor in actors:
            self.add_actor(actor)

    def add_strategy(self, strategy: Strategy) -> None:
        """
        Add the given trading strategy to the trader.

        Parameters
        ----------
        strategy : Strategy
            The trading strategy to add and register.

        Raises
        ------
        ValueError
            If `strategy.state` is ``RUNNING`` or ``DISPOSED``.
        RuntimeError
            If `strategy.id` already exists in the trader.

        """
        PyCondition.not_none(strategy, "strategy")
        PyCondition.true(not strategy.is_running, "strategy.state was RUNNING")
        PyCondition.true(not strategy.is_disposed, "strategy.state was DISPOSED")

        if self.is_running and not self._has_controller:
            self._log.error("Cannot add a strategy to a running trader.")
            return

        if strategy.id in self._strategies:
            raise RuntimeError(
                f"Already registered a strategy with ID {strategy.id}, "
                "try specifying a different strategy ID.",
            )

        if isinstance(self._clock, LiveClock):
            clock = self._clock.__class__(loop=self._loop)
        else:
            clock = self._clock.__class__()

        # Confirm strategy ID
        order_id_tags: list[str] = [s.order_id_tag for s in self._strategies.values()]
        if strategy.order_id_tag in (None, str(None)):
            order_id_tag = f"{len(order_id_tags):03d}"
            # Assign strategy `order_id_tag`
            strategy_id = StrategyId(f"{strategy.id.value.partition('-')[0]}-{order_id_tag}")
            strategy.change_id(strategy_id)
            strategy.change_order_id_tag(order_id_tag)

        # Check for duplicate `order_id_tag`
        if strategy.order_id_tag in order_id_tags:
            raise RuntimeError(
                f"strategy `order_id_tag` conflict for '{strategy.order_id_tag}', "
                f"explicitly define all `order_id_tag` values in your strategy configs",
            )

        # Wire strategy into trader
        strategy.register(
            trader_id=self.id,
            portfolio=self._portfolio,
            msgbus=self._msgbus,
            cache=self._cache,
            clock=clock,  # Clock per strategy
            logger=self._log.get_logger(),
        )

        self._exec_engine.register_oms_type(strategy)
        self._exec_engine.register_external_order_claims(strategy)
        self._strategies[strategy.id] = strategy

        self._log.info(f"Registered Strategy {strategy}.")

    def add_strategies(self, strategies: list[Strategy]) -> None:
        """
        Add the given trading strategies to the trader.

        Parameters
        ----------
        strategies : list[TradingStrategies]
            The trading strategies to add and register.

        Raises
        ------
        ValueError
            If `strategies` is ``None`` or empty.

        """
        PyCondition.not_empty(strategies, "strategies")

        for strategy in strategies:
            self.add_strategy(strategy)

    def add_exec_algorithm(self, exec_algorithm: ExecAlgorithm) -> None:
        """
        Add the given execution algorithm to the trader.

        Parameters
        ----------
        exec_algorithm : ExecAlgorithm
            The execution algorithm to add and register.

        Raises
        ------
        KeyError
            If `exec_algorithm.id` already exists in the trader.
        ValueError
            If `exec_algorithm.state` is ``RUNNING`` or ``DISPOSED``.

        """
        PyCondition.not_none(exec_algorithm, "exec_algorithm")
        PyCondition.true(not exec_algorithm.is_running, "exec_algorithm.state was RUNNING")
        PyCondition.true(not exec_algorithm.is_disposed, "exec_algorithm.state was DISPOSED")

        if self.is_running:
            self._log.error("Cannot add an execution algorithm to a running trader.")
            return

        if exec_algorithm.id in self._exec_algorithms:
            raise RuntimeError(
                f"Already registered an execution algorithm with ID {exec_algorithm.id}, "
                "try specifying a different `exec_algorithm_id`.",
            )

        if isinstance(self._clock, LiveClock):
            clock = self._clock.__class__(loop=self._loop)
        else:
            clock = self._clock.__class__()

        # Wire execution algorithm into trader
        exec_algorithm.register(
            trader_id=self.id,
            portfolio=self._portfolio,
            msgbus=self._msgbus,
            cache=self._cache,
            clock=clock,  # Clock per algorithm
            logger=self._log.get_logger(),
        )

        self._exec_algorithms[exec_algorithm.id] = exec_algorithm

        self._log.info(f"Registered ExecAlgorithm {exec_algorithm}.")

    def add_exec_algorithms(self, exec_algorithms: list[ExecAlgorithm]) -> None:
        """
        Add the given execution algorithms to the trader.

        Parameters
        ----------
        exec_algorithms : list[ExecAlgorithm]
            The execution algorithms to add and register.

        Raises
        ------
        ValueError
            If `exec_algorithms` is ``None`` or empty.

        """
        PyCondition.not_empty(exec_algorithms, "exec_algorithms")

        for exec_algorithm in exec_algorithms:
            self.add_exec_algorithm(exec_algorithm)

    def start_actor(self, actor_id: ComponentId) -> None:
        """
        Start the actor with the given `actor_id`.

        Parameters
        ----------
        actor_id : ComponentId
            The component ID to start.

        Raises
        ------
        ValueError
            If an actor with the given `actor_id` is not found.

        """
        PyCondition.not_none(actor_id, "actor_id")

        actor = self._actors.get(actor_id)
        if actor is None:
            raise ValueError(f"Cannot start actor, {actor_id} not found.")

        if actor.is_running:
            self._log.warning(f"Actor {actor_id} already running.")
            return

        actor.start()

    def start_strategy(self, strategy_id: StrategyId) -> None:
        """
        Start the strategy with the given `strategy_id`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID to start.

        Raises
        ------
        ValueError
            If a strategy with the given `strategy_id` is not found.

        """
        PyCondition.not_none(strategy_id, "strategy_id")

        strategy = self._strategies.get(strategy_id)
        if strategy is None:
            raise ValueError(f"Cannot start strategy, {strategy_id} not found.")

        if strategy.is_running:
            self._log.warning(f"Strategy {strategy_id} already running.")
            return

        strategy.start()

    def stop_actor(self, actor_id: ComponentId) -> None:
        """
        Stop the actor with the given `actor_id`.

        Parameters
        ----------
        actor_id : ComponentId
            The actor ID to stop.

        Raises
        ------
        ValueError
            If an actor with the given `actor_id` is not found.

        """
        PyCondition.not_none(actor_id, "actor_id")

        actor = self._actors.get(actor_id)
        if actor is None:
            raise ValueError(f"Cannot stop actor, {actor_id} not found.")

        if not actor.is_running:
            self._log.warning(f"Actor {actor_id} not running.")
            return

        actor.stop()

    def stop_strategy(self, strategy_id: StrategyId) -> None:
        """
        Stop the strategy with the given `strategy_id`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID to stop.

        Raises
        ------
        ValueError
            If a strategy with the given `strategy_id` is not found.

        """
        PyCondition.not_none(strategy_id, "strategy_id")

        strategy = self._strategies.get(strategy_id)
        if strategy is None:
            raise ValueError(f"Cannot stop strategy, {strategy_id} not found.")

        if not strategy.is_running:
            self._log.warning(f"Strategy {strategy_id} not running.")
            return

        strategy.stop()

    def remove_actor(self, actor_id: ComponentId) -> None:
        """
        Remove the actor with the given `actor_id`.

        Will stop the actor first if state is ``RUNNING``.

        Parameters
        ----------
        actor_id : ComponentId
            The actor ID to remove.

        Raises
        ------
        ValueError
            If an actor with the given `actor_id` is not found.

        """
        PyCondition.not_none(actor_id, "actor_id")

        actor = self._actors.get(actor_id)
        if actor is None:
            raise ValueError(f"Cannot remove actor, {actor_id} not found.")

        if actor.is_running:
            actor.stop()

        self._actors.pop(actor_id)

    def remove_strategy(self, strategy_id: StrategyId) -> None:
        """
        Remove the strategy with the given `strategy_id`.

        Will stop the strategy first if state is ``RUNNING``.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID to remove.

        Raises
        ------
        ValueError
            If a strategy with the given `strategy_id` is not found.

        """
        PyCondition.not_none(strategy_id, "strategy_id")

        strategy = self._strategies.get(strategy_id)
        if strategy is None:
            raise ValueError(f"Cannot remove strategy, {strategy_id} not found.")

        if strategy.is_running:
            strategy.stop()

        self._strategies.pop(strategy_id)

    def clear_actors(self) -> None:
        """
        Dispose and clear all actors held by the trader.

        Raises
        ------
        ValueError
            If state is ``RUNNING``.

        """
        if self.is_running:
            self._log.error("Cannot clear the actors of a running trader.")
            return

        for actor in self._actors.values():
            actor.dispose()

        self._actors.clear()
        self._log.info("Cleared all actors.")

    def clear_strategies(self) -> None:
        """
        Dispose and clear all strategies held by the trader.

        Raises
        ------
        ValueError
            If state is ``RUNNING``.

        """
        if self.is_running:
            self._log.error("Cannot clear the strategies of a running trader.")
            return

        for strategy in self._strategies.values():
            strategy.dispose()

        self._strategies.clear()
        self._log.info("Cleared all trading strategies.")

    def clear_exec_algorithms(self) -> None:
        """
        Dispose and clear all execution algorithms held by the trader.

        Raises
        ------
        ValueError
            If state is ``RUNNING``.

        """
        if self.is_running:
            self._log.error("Cannot clear the execution algorithm of a running trader.")
            return

        for exec_algorithm in self._exec_algorithms.values():
            exec_algorithm.dispose()

        self._exec_algorithms.clear()
        self._log.info("Cleared all execution algorithms.")

    def subscribe(self, topic: str, handler: Callable[[Any], None]) -> None:
        """
        Subscribe to the given message topic with the given callback handler.

        Parameters
        ----------
        topic : str
            The topic for the subscription. May include wildcard glob patterns.
        handler : Callable[[Any], None]
            The handler for the subscription.

        """
        self._msgbus.subscribe(topic=topic, handler=handler)

    def unsubscribe(self, topic: str, handler: Callable[[Any], None]) -> None:
        """
        Unsubscribe the given handler from the given message topic.

        Parameters
        ----------
        topic : str, optional
            The topic to unsubscribe from. May include wildcard glob patterns.
        handler : Callable[[Any], None]
            The handler for the subscription.

        """
        self._msgbus.unsubscribe(topic=topic, handler=handler)

    def save(self) -> None:
        """
        Save all actor and strategy states to the cache.
        """
        for actor in self._actors.values():
            self._cache.update_actor(actor)

        for strategy in self._strategies.values():
            self._cache.update_strategy(strategy)

    def load(self) -> None:
        """
        Load all actor and strategy states from the cache.
        """
        for actor in self._actors.values():
            self._cache.load_actor(actor)

        for strategy in self._strategies.values():
            self._cache.load_strategy(strategy)

    def check_residuals(self) -> None:
        """
        Check for residual open state such as open orders or open positions.
        """
        self._exec_engine.check_residuals()

    def generate_orders_report(self) -> pd.DataFrame:
        """
        Generate an orders report.

        Returns
        -------
        pd.DataFrame

        """
        return ReportProvider.generate_orders_report(self._cache.orders())

    def generate_order_fills_report(self) -> pd.DataFrame:
        """
        Generate an order fills report.

        Returns
        -------
        pd.DataFrame

        """
        return ReportProvider.generate_order_fills_report(self._cache.orders())

    def generate_positions_report(self) -> pd.DataFrame:
        """
        Generate a positions report.

        Returns
        -------
        pd.DataFrame

        """
        positions = self._cache.positions() + self._cache.position_snapshots()
        return ReportProvider.generate_positions_report(positions)

    def generate_account_report(self, venue: Venue) -> pd.DataFrame:
        """
        Generate an account report.

        Returns
        -------
        pd.DataFrame

        """
        account = self._cache.account_for_venue(venue)
        if account is None:
            return pd.DataFrame()
        return ReportProvider.generate_account_report(account)
