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

import pandas as pd

from nautilus_trader.analysis.performance cimport PerformanceAnalyzer
from nautilus_trader.analysis.reports cimport ReportProvider
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
        list strategies not None,
        MessageBus msgbus not None,
        Portfolio portfolio not None,
        DataEngine data_engine not None,
        RiskEngine risk_engine not None,
        ExecutionEngine exec_engine not None,
        Clock clock not None,
        Logger logger not None,
        bint warn_no_strategies=True,
    ):
        """
        Initialize a new instance of the ``Trader`` class.

        Parameters
        ----------
        trader_id : TraderId
            The ID for the trader.
        strategies : list[TradingStrategy]
            The initial strategies for the trader.
        msgbus : MessageBus
            The message bus for the trader.
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
        warn_no_strategies : bool, optional
            If the trader should warn if there are no strategies to initialize.

        Raises
        ------
        ValueError
            If trader_id is not equal to the exec_engine.trader_id.
        ValueError
            If portfolio is not equal to the exec_engine._portfolio.
        ValueError
            If strategies is None.
        ValueError
            If strategies list is empty.
        TypeError
            If strategies list contains a type other than TradingStrategy.

        """
        Condition.equal(trader_id, exec_engine.trader_id, "trader_id", "exec_engine.trader_id")
        super().__init__(clock, logger)

        self._msgbus = msgbus
        self._portfolio = portfolio
        self._data_engine = data_engine
        self._risk_engine = risk_engine
        self._exec_engine = exec_engine
        self._strategies = []
        self._report_provider = ReportProvider()

        self.id = trader_id
        self.analyzer = PerformanceAnalyzer()

        if strategies:
            self.initialize_strategies(
                strategies=strategies,
                warn_no_strategies=warn_no_strategies,
            )
        else:
            if warn_no_strategies:
                self._log.warning(f"No strategies to initialize.")

    cdef list strategies_c(self):
        return self._strategies

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
        Return a dictionary containing the traders strategy states.

        Returns
        -------
        dict[StrategyId, bool]

        """
        cdef dict states = {}
        cdef TradingStrategy strategy
        for strategy in self._strategies:
            states[strategy.id] = strategy.state_string_c()

        return states

# -- ACTION IMPLEMENTATIONS ------------------------------------------------------------------------

    cpdef void _start(self) except *:
        if not self._strategies:
            self._log.error(f"No strategies loaded.")
            return

        cdef TradingStrategy strategy
        for strategy in self._strategies:
            strategy.start()

    cpdef void _stop(self) except *:
        cdef TradingStrategy strategy
        for strategy in self._strategies:
            if strategy.state_c() == ComponentState.RUNNING:
                strategy.stop()
            else:
                self._log.warning(f"{strategy} already stopped.")

    cpdef void _reset(self) except *:
        for strategy in self._strategies:
            strategy.reset()

        self._portfolio.reset()
        self.analyzer.reset()

    cpdef void _dispose(self) except *:
        for strategy in self._strategies:
            strategy.dispose()

# --------------------------------------------------------------------------------------------------

    cpdef void initialize_strategies(
        self,
        list strategies: [TradingStrategy],
        bint warn_no_strategies,
    ) except *:
        """
        Initialize the given strategies.

        Parameters
        ----------
        strategies : list[TradingStrategies]
            The strategies to load into the trader.
        warn_no_strategies : bool
            If the trader should warn if there are no strategies to initialize.

        Raises
        ------
        ValueError
            If strategies is None or empty.
        TypeError
            If strategies contains a type other than TradingStrategy.

        """
        Condition.list_type(strategies, TradingStrategy, "strategies")

        if self._fsm.state == ComponentState.RUNNING:
            self._log.error("Cannot re-initialize the strategies of a running trader.")
            return

        self._log.info(f"Initializing strategies...")

        cdef TradingStrategy strategy
        for strategy in self._strategies:
            Condition.true(strategy.state_c() != ComponentState.RUNNING, "strategy.state_c() was RUNNING")

        # Dispose of current strategies
        for strategy in self._strategies:
            strategy.dispose()

        self._strategies.clear()

        cdef set strategy_ids = set()
        # Initialize strategies
        for strategy in strategies:
            # Check strategy_ids are unique
            if strategy.id not in strategy_ids:
                strategy_ids.add(strategy.id)
            else:
                raise ValueError(f"The strategy_id {strategy.id} was not unique, "
                                 f"duplicate strategy IDs")

            # Wire strategy into trader
            strategy.register(
                trader_id=self.id,
                msgbus=self._msgbus,
                portfolio=self._portfolio,
                data_engine=self._data_engine,
                risk_engine=self._risk_engine,
                clock=self._clock.__class__(),  # Clock per strategy
                logger=self._log.get_logger(),
            )

            # Add to internal strategies
            self._strategies.append(strategy)

            self._log.info(f"Initialized {strategy}.")

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
            self._exec_engine.cache.update_strategy(strategy)

    cpdef void load(self) except *:
        """
        Load all strategy states from the execution cache.
        """
        for strategy in self._strategies:
            self._exec_engine.cache.load_strategy(strategy)

    cpdef object generate_orders_report(self):
        """
        Generate an orders report.

        Returns
        -------
        pd.DataFrame

        """
        return self._report_provider.generate_orders_report(self._exec_engine.cache.orders())

    cpdef object generate_order_fills_report(self):
        """
        Generate an order fills report.

        Returns
        -------
        pd.DataFrame

        """
        return self._report_provider.generate_order_fills_report(self._exec_engine.cache.orders())

    cpdef object generate_positions_report(self):
        """
        Generate a positions report.

        Returns
        -------
        pd.DataFrame

        """
        return self._report_provider.generate_positions_report(self._exec_engine.cache.positions())

    cpdef object generate_account_report(self, Venue venue):
        """
        Generate an account report.

        Returns
        -------
        pd.DataFrame

        """
        cdef Account account = self._exec_engine.cache.account_for_venue(venue)
        if account is None:
            return pd.DataFrame()
        return self._report_provider.generate_account_report(account)
