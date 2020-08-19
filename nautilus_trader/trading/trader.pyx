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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.commands cimport AccountInquiry
from nautilus_trader.common.logging cimport Logger, LoggerAdapter
from nautilus_trader.common.data cimport DataClient
from nautilus_trader.common.execution cimport ExecutionEngine
from nautilus_trader.trading.strategy cimport TradingStrategy
from nautilus_trader.analysis.performance cimport PerformanceAnalyzer
from nautilus_trader.analysis.reports cimport ReportProvider


cdef class Trader:
    """
    Provides a trader for managing a portfolio of trading strategies.
    """

    def __init__(self,
                 TraderId trader_id not None,
                 AccountId account_id not None,
                 list strategies not None,
                 DataClient data_client not None,
                 ExecutionEngine exec_engine not None,
                 Clock clock not None,
                 UUIDFactory uuid_factory not None,
                 Logger logger not None):
        """
        Initialize a new instance of the Trader class.

        :param trader_id: The trader_id for the trader.
        :param trader_id: The account_id for the trader.
        :param strategies: The initial strategies for the trader.
        :param data_client: The data client to register the traders strategies with.
        :param exec_engine: The execution engine to register the traders strategies with trader.
        :param clock: The clock for the trader.
        :param uuid_factory: The uuid_factory for the trader.
        :param logger: The logger for the trader.
        :raises ValueError: If strategies is None.
        :raises ValueError: If strategies list is empty.
        :raises TypeError: If strategies list contains a type other than TradingStrategy.
        :raises ValueError: If trader_id is not equal to the exec_engine.trader_id.
        :raises ValueError: If account_id is not equal to the exec_engine.account_id.
        """
        Condition.equal(trader_id, exec_engine.trader_id, "trader_id", "exec_engine.trader_id")
        Condition.equal(account_id, exec_engine.account_id, "account_id", "exec_engine.account_id")

        self._clock = clock
        self._uuid_factory = uuid_factory
        self.id = trader_id
        self.account_id = account_id
        self._log = LoggerAdapter(f"Trader-{self.id.value}", logger)
        self._data_client = data_client
        self._exec_engine = exec_engine
        self._report_provider = ReportProvider()

        self.portfolio = self._exec_engine.portfolio
        self.analyzer = PerformanceAnalyzer()
        self.is_running = False

        self.strategies = []
        self.initialize_strategies(strategies)

    cpdef void initialize_strategies(self, list strategies: [TradingStrategy]) except *:
        """
        Change strategies with the given list of trading strategies.

        :param strategies: The list of strategies to load into the trader.
        :raises ValueError: If the strategies is None.
        :raises ValueError: If the strategies list is empty.
        :raises TypeError: If the strategies list contains a type other than TradingStrategy.
        """
        Condition.not_empty(strategies, "strategies")
        Condition.list_type(strategies, TradingStrategy, "strategies")

        if self.is_running:
            self._log.error("Cannot re-initialize the strategies of a running trader.")
            return

        for strategy in self.strategies:
            # Design assumption that no strategies are running
            assert not strategy.is_running

        # Check strategy_ids are unique
        strategy_ids = set()
        for strategy in strategies:
            if strategy.id not in strategy_ids:
                strategy_ids.add(strategy.id)
            else:
                raise ValueError(f"The strategy_id {strategy.id} was not unique "
                                 f"(duplicate strategy_ids).")

        # Dispose of current strategies
        for strategy in self.strategies:
            self._exec_engine.deregister_strategy(strategy)
            strategy.dispose()

        self.strategies.clear()

        # Initialize strategies
        for strategy in strategies:
            strategy.change_logger(self._log.get_logger())
            self.strategies.append(strategy)

        for strategy in self.strategies:
            self._data_client.register_strategy(strategy)
            self._exec_engine.register_strategy(strategy)
            self._log.info(f"Initialized {strategy}.")

    cpdef void start(self) except *:
        """
        Start the trader.
        """
        if self.is_running:
            self._log.error(f"Cannot start trader (already running).")
            return

        if not self.strategies:
            self._log.error(f"Cannot start trader (no strategies loaded).")
            return

        self._log.info("Starting...")
        self.account_inquiry()

        for strategy in self.strategies:
            strategy.start()

        self.is_running = True
        self._log.info("Running...")

    cpdef void stop(self) except *:
        """
        Stop the trader.
        """
        if not self.is_running:
            self._log.error(f"Cannot stop trader (already stopped).")
            return

        self._log.debug("Stopping...")
        for strategy in self.strategies:
            strategy.stop()

        self.is_running = False
        self._log.info("Stopped.")

    cpdef void check_residuals(self) except *:
        """
        Check for residual business objects such as working orders or open positions.
        """
        self._exec_engine.check_residuals()

    cpdef void save(self) except *:
        """
        Save all strategy states to the execution database.
        """
        for strategy in self.strategies:
            self._exec_engine.database.update_strategy(strategy)

    cpdef void load(self) except *:
        """
        Load all strategy states from the execution database.
        """
        for strategy in self.strategies:
            self._exec_engine.database.load_strategy(strategy)

    cpdef void reset(self) except *:
        """
        Reset the trader.

        All stateful values of the portfolio, and every strategy are reset.

        Note: The trader cannot be running otherwise an error is logged.
        """
        if self.is_running:
            self._log.error(f"Cannot reset trader (trader must be stopped to reset).")
            return

        self._log.debug("Resetting...")

        for strategy in self.strategies:
            strategy.reset()

        self.portfolio.reset()
        self.analyzer.reset()
        self.is_running = False

        self._log.info("Reset.")

    cpdef void dispose(self) except *:
        """
        Dispose of the trader.

        Disposes all internally held strategies.
        """
        self._log.debug("Disposing...")
        for strategy in self.strategies:
            strategy.dispose()

        self._log.info("Disposed.")

    cpdef void account_inquiry(self) except *:
        """
        Send an AccountInquiry command to the execution service.
        """
        cdef AccountInquiry command = AccountInquiry(
            trader_id=self.id,
            account_id=self.account_id,
            command_id=self._uuid_factory.generate(),
            command_timestamp=self._clock.time_now())

        self._exec_engine.execute_command(command)

    cpdef dict strategy_status(self):
        """
        Return a dictionary containing the traders strategy status.
        The key is the strategy_id.
        The value is a bool which is True if the strategy is running else False.

        :return Dict[StrategyId, bool].
        """
        cdef status = {}
        for strategy in self.strategies:
            if strategy.is_running:
                status[strategy.id] = True
            else:
                status[strategy.id] = False

        return status

    cpdef object generate_orders_report(self):
        """
        Return an orders report dataframe.

        :return pd.DataFrame.
        """
        return self._report_provider.generate_orders_report(self._exec_engine.database.get_orders())

    cpdef object generate_order_fills_report(self):
        """
        Return an order fills report dataframe.

        :return pd.DataFrame.
        """
        return self._report_provider.generate_order_fills_report(self._exec_engine.database.get_orders())

    cpdef object generate_positions_report(self):
        """
        Return a positions report dataframe.

        :return pd.DataFrame.
        """
        return self._report_provider.generate_positions_report(self._exec_engine.database.get_positions())

    cpdef object generate_account_report(self):
        """
        Return an account report dataframe.

        :return pd.DataFrame.
        """
        return self._report_provider.generate_account_report(self._exec_engine.account.get_events())
