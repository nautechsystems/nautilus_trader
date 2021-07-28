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

"""
The `Trader` class is intended to manage a portfolio of trading strategies within
a running instance of the platform.

A running instance could be either a test/backtest or live implementation - the
`Trader` will operate in the same way.
"""

from typing import Any
from typing import Callable

import pandas as pd

from nautilus_trader.analysis.performance cimport PerformanceAnalyzer
from nautilus_trader.analysis.reports cimport ReportProvider
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.c_enums.component_state cimport ComponentState
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.msgbus.message_bus cimport MessageBus
from nautilus_trader.risk.engine cimport RiskEngine
from nautilus_trader.trading.account cimport Account
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class Trader(Component):
    """
    Provides a trader for managing a portfolio of trading strategies.
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
    ):
        """
        Initialize a new instance of the ``Trader`` class.

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

        Raises
        ------
        ValueError
            If portfolio is not equal to the exec_engine._portfolio.
        ValueError
            If strategies is None.
        ValueError
            If strategies list is empty.
        TypeError
            If strategies list contains a type other than TradingStrategy.

        """
        super().__init__(clock, logger)

        self._msgbus = msgbus
        self._cache = cache
        self._portfolio = portfolio
        self._data_engine = data_engine
        self._risk_engine = risk_engine
        self._exec_engine = exec_engine
        self._report_provider = ReportProvider()

        self._strategies = []
        self._plugins = []

        self.id = trader_id
        self.analyzer = PerformanceAnalyzer()

    cdef list strategies_c(self):
        return self._strategies

    cdef list plugins_c(self):
        return self._plugins

    cpdef list strategy_ids(self):
        """
        Return the strategy IDs loaded in the trader.

        Returns
        -------
        list[StrategyId]

        """
        return sorted([strategy.id for strategy in self._strategies])

    cpdef dict strategy_states(self):
        """
        Return the traders strategy states.

        Returns
        -------
        dict[StrategyId, bool]

        """
        cdef dict states = {}
        cdef TradingStrategy strategy
        for strategy in self._strategies:
            states[strategy.id] = strategy.state_string_c()

        return states

    cpdef list plugins(self):
        """
        Return the plugins loaded in the trader.

        Returns
        -------
        list[Actor]

        """
        return self._plugins.copy()

# -- ACTION IMPLEMENTATIONS ------------------------------------------------------------------------

    cpdef void _start(self) except *:
        if not self._strategies:
            self._log.error(f"No strategies loaded.")
            return

        cdef TradingStrategy strategy
        for strategy in self._strategies:
            strategy.start()

        for plugin in self._plugins:
            plugin.start()

    cpdef void _stop(self) except *:
        cdef TradingStrategy strategy
        for strategy in self._strategies:
            if strategy.state_c() == ComponentState.RUNNING:
                strategy.stop()
            else:
                self._log.warning(f"{strategy} already stopped.")

        for plugin in self._plugins:
            if plugin.state_c() == ComponentState.RUNNING:
                plugin.stop()
            else:
                self._log.warning(f"{plugin} already stopped.")

    cpdef void _reset(self) except *:
        for strategy in self._strategies:
            strategy.reset()

        for plugin in self._plugins:
            plugin.reset()

        self._portfolio.reset()
        self.analyzer.reset()

    cpdef void _dispose(self) except *:
        for strategy in self._strategies:
            strategy.dispose()

        for plugin in self._plugins:
            plugin.dispose()

# --------------------------------------------------------------------------------------------------

    cpdef void add_strategy(self, TradingStrategy strategy) except *:
        """
        Add the given trading strategy to the trader.

        Parameters
        ----------
        strategy : TradingStrategy
            The trading strategy to add and register.

        Raises
        ------
        KeyError
            If strategy.id already exists in the trader.
        ValueError
            If strategy.state is `RUNNING` or `DISPOSED`.

        """
        Condition.not_none(strategy, "strategy")
        Condition.not_in(strategy, self._strategies, "strategy", "strategies")
        Condition.true(strategy.state_c() != ComponentState.RUNNING, "strategy.state_c() was RUNNING")
        Condition.true(strategy.state_c() != ComponentState.DISPOSED, "strategy.state_c() was DISPOSED")

        if self._fsm.state == ComponentState.RUNNING:
            self._log.error("Cannot add a strategy to a running trader.")
            return

        # Wire strategy into trader
        strategy.register(
            trader_id=self.id,
            portfolio=self._portfolio,
            msgbus=self._msgbus,
            cache=self._cache,
            clock=self._clock.__class__(),  # Clock per strategy
            logger=self._log.get_logger(),
        )

        self._strategies.append(strategy)

        self._log.info(f"Registered {strategy}.")

    cpdef void add_strategies(self, list strategies: [TradingStrategy]) except *:
        """
        Add the given trading strategies to the trader.

        Parameters
        ----------
        strategies : list[TradingStrategies]
            The trading strategies to add and register.

        Raises
        ------
        ValueError
            If strategies is None or empty.
        TypeError
            If strategies contains a type other than TradingStrategy.

        """
        Condition.not_empty(strategies, "strategies")
        Condition.list_type(strategies, TradingStrategy, "strategies")

        for strategy in strategies:
            self.add_strategy(strategy)

    cpdef void add_plugin(self, Actor plugin) except *:
        """
        Add the given plugin component to the trader.

        Parameters
        ----------
        plugin : Actor
            The plugin component to add and register.

        """
        if self._fsm.state == ComponentState.RUNNING:
            self._log.error("Cannot add plugin to a running trader.")
            return

        # Wire plugin into trader
        plugin.register_base(
            trader_id=self.id,
            msgbus=self._msgbus,
            cache=self._cache,
            clock=self._clock.__class__(),  # Clock per plugin
            logger=self._log.get_logger(),
        )

        self._plugins.append(plugin)

        self._log.info(f"Registered {plugin}.")

    cpdef void add_plugins(self, list plugins: [Actor]) except *:
        """
        Add the given plugin components to the trader.

        Parameters
        ----------
        plugins : list[TradingStrategies]
            The plugin components to add and register.

        Raises
        ------
        ValueError
            If strategies is None or empty.
        TypeError
            If strategies contains a type other than TradingStrategy.

        """
        Condition.not_empty(plugins, "plugins")
        Condition.list_type(plugins, Actor, "plugins")

        for plugin in plugins:
            self.add_plugin(plugin)

    cpdef void clear_strategies(self) except *:
        """
        Dispose and clear all strategies held by the trader.

        Raises
        ------
        ValueError
            If state is RUNNING.

        """
        if self._fsm.state == ComponentState.RUNNING:
            self._log.error("Cannot clear the strategies of a running trader.")
            return

        for strategy in self._strategies:
            strategy.dispose()

        self._strategies.clear()

    cpdef void clear_plugins(self) except *:
        """
        Dispose and clear all plugins held by the trader.

        Raises
        ------
        ValueError
            If state is RUNNING.

        """
        if self._fsm.state == ComponentState.RUNNING:
            self._log.error("Cannot clear the strategies of a running trader.")
            return

        for plugin in self._plugins:
            plugin.dispose()

        self._plugins.clear()

    cpdef void subscribe(self, str topic, handler: Callable[[Any], None]) except *:
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

    cpdef void unsubscribe(self, str topic, handler: Callable[[Any], None]) except *:
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

    cpdef void check_residuals(self) except *:
        """
        Check for residual business objects such as working orders or open positions.
        """
        self._exec_engine.check_residuals()

    cpdef void save(self) except *:
        """
        Save all strategy states to the execution cache.
        """
        for strategy in self._strategies:
            self._cache.update_strategy(strategy)

    cpdef void load(self) except *:
        """
        Load all strategy states from the execution cache.
        """
        for strategy in self._strategies:
            self._cache.load_strategy(strategy)

    cpdef object generate_orders_report(self):
        """
        Generate an orders report.

        Returns
        -------
        pd.DataFrame

        """
        return self._report_provider.generate_orders_report(self._cache.orders())

    cpdef object generate_order_fills_report(self):
        """
        Generate an order fills report.

        Returns
        -------
        pd.DataFrame

        """
        return self._report_provider.generate_order_fills_report(self._cache.orders())

    cpdef object generate_positions_report(self):
        """
        Generate a positions report.

        Returns
        -------
        pd.DataFrame

        """
        return self._report_provider.generate_positions_report(self._cache.positions())

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
        return self._report_provider.generate_account_report(account)
