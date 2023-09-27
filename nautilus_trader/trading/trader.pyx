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
The `Trader` class is intended to manage a fleet of trading strategies within
a running instance of the platform.

A running instance could be either a test/backtest or live implementation - the
`Trader` will operate in the same way.
"""

import asyncio
from typing import Any, Callable, Optional

import pandas as pd

from nautilus_trader.analysis.reporter import ReportProvider

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.execution.algorithm cimport ExecAlgorithm
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.identifiers cimport ExecAlgorithmId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.msgbus.bus cimport MessageBus
from nautilus_trader.risk.engine cimport RiskEngine
from nautilus_trader.trading.strategy cimport Strategy


cdef class Trader(Component):
    """
    Provides a trader for managing a fleet of trading strategies.

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
        TraderId trader_id not None,
        MessageBus msgbus not None,
        Cache cache not None,
        Portfolio portfolio not None,
        DataEngine data_engine not None,
        RiskEngine risk_engine not None,
        ExecutionEngine exec_engine not None,
        Clock clock not None,
        Logger logger not None,
        loop: Optional[asyncio.AbstractEventLoop] = None,
        dict config = None,
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

        self._actors = []
        self._strategies = []
        self._exec_algorithms = []
        self._has_controller = config.get("has_controller")

    cpdef list actors(self):
        """
        Return the actors loaded in the trader.

        Returns
        -------
        list[Actor]

        """
        return self._actors

    cpdef list strategies(self):
        """
        Return the strategies loaded in the trader.

        Returns
        -------
        list[Strategy]

        """
        return self._strategies

    cpdef list exec_algorithms(self):
        """
        Return the execution algorithms loaded in the trader.

        Returns
        -------
        list[ExecAlgorithms]

        """
        return self._exec_algorithms

    cpdef list actor_ids(self):
        """
        Return the actor IDs loaded in the trader.

        Returns
        -------
        list[ComponentId]

        """
        return sorted([actor.id for actor in self._actors])

    cpdef list strategy_ids(self):
        """
        Return the strategy IDs loaded in the trader.

        Returns
        -------
        list[StrategyId]

        """
        return sorted([s.id for s in self._strategies])

    cpdef list exec_algorithm_ids(self):
        """
        Return the execution algorithm IDs loaded in the trader.

        Returns
        -------
        list[ExecAlgorithmId]

        """
        return sorted([e.id for e in self._exec_algorithms])

    cpdef dict actor_states(self):
        """
        Return the traders actor states.

        Returns
        -------
        dict[ComponentId, str]

        """
        cdef Actor a
        return {a.id: a.state.name for a in self._actors}

    cpdef dict strategy_states(self):
        """
        Return the traders strategy states.

        Returns
        -------
        dict[StrategyId, str]

        """
        cdef Strategy s
        return {s.id: s.state.name for s in self._strategies}

    cpdef dict exec_algorithm_states(self):
        """
        Return the traders execution algorithm states.

        Returns
        -------
        dict[ExecAlgorithmId, str]

        """
        cdef ExecAlgorithm s
        return {e.id: e.state.name for e in self._exec_algorithms}

# -- ACTION IMPLEMENTATIONS -----------------------------------------------------------------------

    cpdef void _start(self):
        if not self._strategies:
            self._log.warning(f"No strategies loaded.")

        cdef Actor actor
        for actor in self._actors:
            actor.start()

        cdef Strategy strategy
        for strategy in self._strategies:
            strategy.start()

        cdef ExecAlgorithm exec_algorithm
        for exec_algorithm in self._exec_algorithms:
            exec_algorithm.start()

    cpdef void _stop(self):
        cdef Actor actor
        for actor in self._actors:
            if actor.is_running:
                actor.stop()
            else:
                self._log.warning(f"{actor} already stopped.")

        cdef Strategy strategy
        for strategy in self._strategies:
            if strategy.is_running:
                strategy.stop()
            else:
                self._log.warning(f"{strategy} already stopped.")

        cdef ExecAlgorithm exec_algorithm
        for exec_algorithm in self._exec_algorithms:
            if exec_algorithm.is_running:
                exec_algorithm.stop()
            else:
                self._log.warning(f"{exec_algorithm} already stopped.")

    cpdef void _reset(self):
        cdef Actor actor
        for actor in self._actors:
            actor.reset()

        cdef Strategy strategy
        for strategy in self._strategies:
            strategy.reset()

        cdef ExecAlgorithm exec_algorithm
        for exec_algorithm in self._exec_algorithms:
            exec_algorithm.reset()

        self._portfolio.reset()

    cpdef void _dispose(self):
        cdef Actor actor
        for actor in self._actors:
            actor.dispose()

        cdef Strategy strategy
        for strategy in self._strategies:
            strategy.dispose()

        cdef ExecAlgorithm exec_algorithm
        for exec_algorithm in self._exec_algorithms:
            exec_algorithm.dispose()

# --------------------------------------------------------------------------------------------------

    cpdef void add_actor(self, Actor actor):
        """
        Add the given custom component to the trader.

        Parameters
        ----------
        actor : Actor
            The actor to add and register.

        Raises
        ------
        KeyError
            If `component.id` already exists in the trader.
        ValueError
            If `component.state` is ``RUNNING`` or ``DISPOSED``.

        """
        Condition.true(not actor.is_running, "actor.state was RUNNING")
        Condition.true(not actor.is_disposed, "actor.state was DISPOSED")

        if self.is_running:
            self._log.error("Cannot add component to a running trader.")
            return

        if actor in self._actors:
            raise RuntimeError(
                f"Already registered an actor with ID {actor.id}, "
                "try specifying a different `component_id`."
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

        self._actors.append(actor)

        self._log.info(f"Registered Component {actor}.")

    cpdef void add_actors(self, list actors: [Actor]):
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
        Condition.not_empty(actors, "actors")

        cdef Actor actor
        for actor in actors:
            self.add_actor(actor)

    cpdef void add_strategy(self, Strategy strategy):
        """
        Add the given trading strategy to the trader.

        Parameters
        ----------
        strategy : Strategy
            The trading strategy to add and register.

        Raises
        ------
        KeyError
            If `strategy.id` already exists in the trader.
        ValueError
            If `strategy.state` is ``RUNNING`` or ``DISPOSED``.

        """
        Condition.not_none(strategy, "strategy")
        Condition.true(not strategy.is_running, "strategy.state was RUNNING")
        Condition.true(not strategy.is_disposed, "strategy.state was DISPOSED")

        if self.is_running and not self._has_controller:
            self._log.error("Cannot add a strategy to a running trader.")
            return

        if strategy in self._strategies:
            raise RuntimeError(
                f"Already registered a strategy with ID {strategy.id}, "
                "try specifying a different `strategy_id`."
            )

        if isinstance(self._clock, LiveClock):
            clock = self._clock.__class__(loop=self._loop)
        else:
            clock = self._clock.__class__()

        # Confirm strategy ID
        order_id_tags: list[str] = [s.order_id_tag for s in self._strategies]
        if strategy.order_id_tag in (None, str(None)):
            order_id_tag = f"{len(order_id_tags):03d}"
            # Assign strategy `order_id_tag`
            strategy.id = StrategyId(f"{strategy.id.value.partition('-')[0]}-{order_id_tag}")
            strategy.order_id_tag = order_id_tag

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
        self._strategies.append(strategy)

        self._log.info(f"Registered Strategy {strategy}.")

    cpdef void add_strategies(self, list strategies: [Strategy]):
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
        Condition.not_empty(strategies, "strategies")

        cdef Strategy strategy
        for strategy in strategies:
            self.add_strategy(strategy)

    cpdef void add_exec_algorithm(self, ExecAlgorithm exec_algorithm):
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
        Condition.not_none(exec_algorithm, "exec_algorithm")
        Condition.true(not exec_algorithm.is_running, "exec_algorithm.state was RUNNING")
        Condition.true(not exec_algorithm.is_disposed, "exec_algorithm.state was DISPOSED")

        if self.is_running:
            self._log.error("Cannot add an execution algorithm to a running trader.")
            return

        if exec_algorithm in self._exec_algorithms:
            raise RuntimeError(
                f"Already registered an execution algorithm with ID {exec_algorithm.id}, "
                "try specifying a different `exec_algorithm_id`."
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

        self._exec_algorithms.append(exec_algorithm)

        self._log.info(f"Registered ExecAlgorithm {exec_algorithm}.")

    cpdef void add_exec_algorithms(self, list exec_algorithms: [ExecAlgorithm]):
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
        Condition.not_empty(exec_algorithms, "exec_algorithms")

        cdef ExecAlgorithm exec_algorithm
        for exec_algorithm in exec_algorithms:
            self.add_exec_algorithm(exec_algorithm)

    cpdef void clear_actors(self):
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

        for actor in self._actors:
            actor.dispose()

        self._actors.clear()
        self._log.info(f"Cleared all actors.")

    cpdef void clear_strategies(self):
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

        for strategy in self._strategies:
            strategy.dispose()

        self._strategies.clear()
        self._log.info(f"Cleared all trading strategies.")

    cpdef void clear_exec_algorithms(self):
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

        for exec_algorithm in self._exec_algorithms:
            exec_algorithm.dispose()

        self._exec_algorithms.clear()
        self._log.info(f"Cleared all execution algorithms.")

    cpdef void subscribe(self, str topic, handler: Callable[[Any], None]):
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

    cpdef void unsubscribe(self, str topic, handler: Callable[[Any], None]):
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

    cpdef void save(self):
        """
        Save all actor and strategy states to the cache.
        """
        cdef Actor actor
        for actor in self._actors:
            self._cache.update_actor(actor)

        cdef Strategy strategy
        for strategy in self._strategies:
            self._cache.update_strategy(strategy)

    cpdef void load(self):
        """
        Load all actor and strategy states from the cache.
        """
        cdef Actor actor
        for actor in self._actors:
            self._cache.load_actor(actor)

        cdef Strategy strategy
        for strategy in self._strategies:
            self._cache.load_strategy(strategy)

    cpdef void check_residuals(self):
        """
        Check for residual open state such as open orders or open positions.
        """
        self._exec_engine.check_residuals()

    cpdef object generate_orders_report(self):
        """
        Generate an orders report.

        Returns
        -------
        pd.DataFrame

        """
        return ReportProvider.generate_orders_report(self._cache.orders())

    cpdef object generate_order_fills_report(self):
        """
        Generate an order fills report.

        Returns
        -------
        pd.DataFrame

        """
        return ReportProvider.generate_order_fills_report(self._cache.orders())

    cpdef object generate_positions_report(self):
        """
        Generate a positions report.

        Returns
        -------
        pd.DataFrame

        """
        cdef list positions = self._cache.positions() + self._cache.position_snapshots()
        return ReportProvider.generate_positions_report(positions)

    cpdef object generate_account_report(self, Venue venue):
        """
        Generate an account report.

        Returns
        -------
        pd.DataFrame

        """
        cdef Account account = self._cache.account_for_venue(venue)
        if account is None:
            return pd.DataFrame()
        return ReportProvider.generate_account_report(account)
