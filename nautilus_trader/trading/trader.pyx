# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.analysis.performance cimport PerformanceAnalyzer
from nautilus_trader.analysis.reports cimport ReportProvider
from nautilus_trader.common.c_enums.component_state cimport ComponentState
from nautilus_trader.common.c_enums.component_trigger cimport ComponentTrigger
from nautilus_trader.common.component cimport ComponentFSMFactory
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.fsm cimport InvalidStateTrigger
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class Trader:
    """
    Provides a trader for managing a portfolio of trading strategies.
    """

    def __init__(
            self,
            TraderId trader_id not None,
            list strategies not None,
            DataEngine data_engine not None,
            ExecutionEngine exec_engine not None,
            Clock clock not None,
            UUIDFactory uuid_factory not None,
            Logger logger not None,
    ):
        """
        Initialize a new instance of the `Trader` class.

        Parameters
        ----------
        trader_id : TraderId
            The identifier for the trader.
        strategies : list[TradingStrategy]
            The initial strategies for the trader.
        data_engine : DataEngine
            The data engine to register the traders strategies with.
        exec_engine : ExecutionEngine
            The execution engine to register the traders strategies with.
        clock : Clock
            The clock for the trader.
        uuid_factory : UUIDFactory
            The uuid_factory for the trader.
        logger : Logger
            The logger for the trader.

        Raises
        ------
        ValueError
            If strategies is None.
        ValueError
            If strategies list is empty.
        TypeError
            If strategies list contains a type other than TradingStrategy.
        ValueError
            If trader_id is not equal to the exec_engine.trader_id.
        ValueError
            If account_id is not equal to the exec_engine.account_id.

        """
        Condition.equal(trader_id, exec_engine.trader_id, "trader_id", "exec_engine.trader_id")

        # Core components
        self._clock = clock
        self._uuid_factory = uuid_factory
        self._log = LoggerAdapter(f"Trader-{trader_id.value}", logger)
        self._fsm = ComponentFSMFactory.create()

        # Private components
        self._data_engine = data_engine
        self._exec_engine = exec_engine
        self._report_provider = ReportProvider()
        self._strategies = []

        self.id = trader_id
        self.portfolio = exec_engine.portfolio
        self.analyzer = PerformanceAnalyzer()

        self.initialize_strategies(strategies)

    cdef ComponentState state_c(self) except *:
        return <ComponentState>self._fsm.state

    cdef str state_string_c(self):
        return self._fsm.state_string_c()

    cdef list strategies_c(self):
        return self._strategies

    @property
    def state(self):
        """
        Returns
        -------
        ComponentState
            The traders current state.

        """
        return self.state_c()

    cpdef list strategy_ids(self):
        """
        The traders strategy identifiers.

        Returns
        -------
        list[StrategyId]

        """
        return sorted([strategy.id for strategy in self._strategies])

    cpdef void initialize_strategies(self, list strategies: [TradingStrategy]) except *:
        """
        Change strategies with the given list of trading strategies.

        Parameters
        ----------
        strategies : list[TradingStrategies]
            The strategies to load into the trader.

        Raises
        ------
        ValueError
            If strategies is None or empty.
        TypeError
            If strategies contains a type other than TradingStrategy.

        """
        Condition.not_empty(strategies, "strategies")
        Condition.list_type(strategies, TradingStrategy, "strategies")

        if self._fsm.state == ComponentState.RUNNING:
            self._log.error("Cannot re-initialize the strategies of a running trader.")
            return

        self._log.debug(f"Initializing strategies...")

        cdef TradingStrategy strategy
        for strategy in self._strategies:
            Condition.true(strategy.state_c() != ComponentState.RUNNING, "strategy.state_c() != RUNNING")

        # Dispose of current strategies
        for strategy in self._strategies:
            self._exec_engine.deregister_strategy(strategy)
            strategy.dispose()

        self._strategies.clear()

        cdef set strategy_ids = set()
        # Initialize strategies
        for strategy in strategies:
            # Check strategy_ids are unique
            if strategy.id not in strategy_ids:
                strategy_ids.add(strategy.id)
            else:
                raise ValueError(f"The strategy_id {strategy.id} was not unique "
                                 f"(duplicate strategy identifiers)")

            # Wire trader into strategy
            strategy.register_trader(
                self.id,
                self._clock.__class__(),  # Clock per strategy
                self._log.get_logger(),
            )

            # Wire data engine into strategy
            self._data_engine.register_strategy(strategy)

            # Wire execution engine into strategy
            self._exec_engine.register_strategy(strategy)

            # Add to internal strategies
            self._strategies.append(strategy)

            self._log.info(f"Initialized {strategy}.")

    cpdef void start(self) except *:
        """
        Start the trader.
        """
        try:
            self._fsm.trigger(ComponentTrigger.START)
        except InvalidStateTrigger as ex:
            self._log.exception(ex)
            raise ex  # Do not put trader in an invalid state

        self._log.info(f"state={self._fsm.state_string_c()}...")

        if not self._strategies:
            self._log.error(f"Cannot start trader (no strategies loaded).")
            return

        cdef TradingStrategy strategy
        for strategy in self._strategies:
            strategy.start()

        self._fsm.trigger(ComponentTrigger.RUNNING)
        self._log.info(f"state={self._fsm.state_string_c()}.")

    cpdef void stop(self) except *:
        """
        Stop the trader.
        """
        try:
            self._fsm.trigger(ComponentTrigger.STOP)
        except InvalidStateTrigger as ex:
            self._log.exception(ex)
            raise ex  # Do not put trader in an invalid state

        self._log.info(f"state={self._fsm.state_string_c()}...")

        cdef TradingStrategy strategy
        for strategy in self._strategies:
            if strategy.state_c() == ComponentState.RUNNING:
                strategy.stop()
            else:
                self._log.warning(f"{strategy} already stopped.")

        self._fsm.trigger(ComponentTrigger.STOPPED)
        self._log.info(f"state={self._fsm.state_string_c()}.")

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

    cpdef void reset(self) except *:
        """
        Reset the trader.

        All stateful values of the portfolio, and every strategy are reset.

        Raises
        ------
        InvalidStateTrigger
            If trader state is RUNNING.

        """
        try:
            self._fsm.trigger(ComponentTrigger.RESET)
        except InvalidStateTrigger as ex:
            self._log.exception(ex)
            raise ex  # Do not put trader in an invalid state

        self._log.info(f"state={self._fsm.state_string_c()}...")

        for strategy in self._strategies:
            strategy.reset()

        self.portfolio.reset()
        self.analyzer.reset()

        self._fsm.trigger(ComponentTrigger.RESET)  # State changes to initialized
        self._log.info(f"state={self._fsm.state_string_c()}.")

    cpdef void dispose(self) except *:
        """
        Dispose of the trader.

        Disposes all internally held strategies.

        This method is idempotent and irreversible. No other methods should be
        called after disposal.
        """
        try:
            self._fsm.trigger(ComponentTrigger.DISPOSE)
        except InvalidStateTrigger as ex:
            self._log.exception(ex)
            raise ex  # Do not put trader in an invalid state

        self._log.info(f"state={self._fsm.state_string_c()}...")

        for strategy in self._strategies:
            strategy.dispose()

        self._fsm.trigger(ComponentTrigger.DISPOSED)
        self._log.info(f"state={self._fsm.state_string_c()}.")

    cpdef dict strategy_states(self):
        """
        Return a dictionary containing the traders strategy states.

        The key is the strategy_id.

        Returns
        -------
        dict[StrategyId, bool]

        """
        cdef dict states = {}
        cdef TradingStrategy strategy
        for strategy in self._strategies:
            states[strategy.id] = strategy.state_string_c()

        return states

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

    cpdef object generate_account_report(self, AccountId account_id):
        """
        Generate an account report.

        Returns
        -------
        pd.DataFrame

        """
        return self._report_provider.generate_account_report(self._exec_engine.cache.account(account_id))
