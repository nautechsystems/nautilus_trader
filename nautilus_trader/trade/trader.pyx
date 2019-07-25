#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="trader.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime
from typing import List

from nautilus_trader.core.precondition cimport Precondition
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.common.account cimport Account
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logger cimport Logger, LoggerAdapter, LiveLogger
from nautilus_trader.common.data cimport DataClient
from nautilus_trader.common.execution cimport ExecutionClient
from nautilus_trader.trade.portfolio cimport Portfolio
from nautilus_trader.trade.strategy cimport TradeStrategy
from nautilus_trader.trade.reports cimport ReportProvider


cdef class Trader:
    """
    Provides a trader for managing a portfolio of trade strategies.
    """

    def __init__(self,
                 str id_tag_trader,
                 list strategies,
                 DataClient data_client,
                 ExecutionClient exec_client,
                 Account account,
                 Portfolio portfolio,
                 Clock clock=LiveClock(),
                 Logger logger=LiveLogger()):
        """
        Initializes a new instance of the Trader class.

        :param id_tag_trader: The identifier tag for the trader (unique at fund level).
        :param strategies: The initial list of strategies to manage.
        :param logger: The logger for the trader.
        :raises ValueError: If the label is an invalid string.
        :raises ValueError: If the strategies list is empty.
        :raises ValueError: If the strategies list contains a type other than TradeStrategy.
        :raises ValueError: If the data client is None.
        :raises ValueError: If the exec client is None.
        :raises ValueError: If the clock is None.
        """
        Precondition.valid_string(id_tag_trader, 'id_tag_trader')
        Precondition.not_empty(strategies, 'strategies')
        Precondition.list_type(strategies, TradeStrategy, 'strategies')
        Precondition.not_none(data_client, 'data_client')
        Precondition.not_none(exec_client, 'exec_client')
        Precondition.not_none(clock, 'clock')

        self._clock = clock
        self.id = TraderId(self.__class__.__name__ + '-' + id_tag_trader)
        self.id_tag_trader = ValidString(id_tag_trader)
        self._log = LoggerAdapter(f"{self.id.value}", logger)
        self._data_client = data_client
        self._exec_client = exec_client
        self._report_provider = ReportProvider()

        self.account = account
        self.portfolio = portfolio
        self.portfolio.register_execution_client(self._exec_client)
        self.is_running = False
        self.started_datetimes = []  # type: List[datetime]
        self.stopped_datetimes = []  # type: List[datetime]

        self.strategies = strategies
        self._initialize_strategies()

    cdef _initialize_strategies(self):
        for strategy in self.strategies:
            strategy.register_trader_id(self.id, self.id_tag_trader)
            self._data_client.register_strategy(strategy)
            self._exec_client.register_strategy(strategy)
            self._log.info(f"Initialized {strategy}.")

    cpdef start(self):
        """
        Start the trader.
        """
        if self.is_running:
            self._log.error(f"Cannot start an already running Trader...")
            return

        self.started_datetimes.append(self._clock.time_now())

        self._log.info("Starting...")
        self._data_client.connect()
        self._exec_client.connect()

        self._data_client.update_all_instruments()

        for strategy in self.strategies:
            strategy.start()

        self.is_running = True
        self._log.info("Running...")

    cpdef stop(self):
        """
        Stop the trader.
        """
        if not self.is_running:
            self._log.error(f"Cannot stop an already stopped Trader...")
            return

        self.stopped_datetimes.append(self._clock.time_now())

        self._log.info("Stopping...")
        for strategy in self.strategies:
            strategy.stop()

        self.is_running = False
        self._log.info("Stopped.")
        self._exec_client.check_residuals()
        self.portfolio.check_residuals()

    cpdef reset(self):
        """
        Reset the trader by returning all stateful internal values of the portfolio, 
        and every strategy to their initial value.
        
        Note: The trader cannot be running otherwise an error is logged.
        """
        if self.is_running:
            self._log.error(f"Cannot reset a running Trader...")
            return

        self._log.info("Resetting...")
        for strategy in self.strategies:
            strategy.reset()

        self.portfolio.reset()
        self._log.info("Reset.")

    cpdef dispose(self):
        """
        Dispose of the trader.
        """
        self._log.info("Disposing...")
        for strategy in self.strategies:
            strategy.dispose()

        self._data_client.disconnect()
        self._exec_client.disconnect()
        self._log.info("Disposed.")

    cpdef change_strategies(self, list strategies: List[TradeStrategy]):
        """
        Change strategies with the given list of trade strategies.
        
        :param strategies: The list of strategies to load into the trader.
        :raises ValueError: If the strategies list is empty.
        :raises ValueError: If the strategies list contains a type other than TradeStrategy.
        """
        Precondition.not_empty(strategies, 'strategies')
        Precondition.list_type(strategies, TradeStrategy, 'strategies')

        if self.is_running:
            self._log.error('Cannot change the strategies of a running trader.')
            return

        # Check strategy identifiers are unique
        strategy_ids = []   # type: List[StrategyId]
        for strategy in strategies:
            if strategy.id not in strategy_ids:
                strategy_ids.append(strategy.id)
            else:
                raise RuntimeError(f'The strategy identifier {strategy.id} is not unique.')

        for strategy in self.strategies:
            self._exec_client.deregister_strategy(strategy)
            strategy.dispose()

        self.strategies = strategies
        self._initialize_strategies()

    cpdef dict strategy_status(self):
        """
        Return a dictionary containing the traders strategy status.
        The key is the strategy identifier.
        The value is a bool which is True if the strategy is running else False.
        
        :return: Dict[StrategyId, bool].
        """
        cdef status = {}
        for strategy in self.strategies:
            if strategy.is_running:
                status[strategy.id] = True
            else:
                status[strategy.id] = False

        return status

    cpdef void create_returns_tear_sheet(self):
        """
        Create a returns tear sheet based on analyzer data from the last run.
        """
        self.portfolio.analyzer.create_returns_tear_sheet()

    cpdef void create_full_tear_sheet(self):
        """
        Create a full tear sheet based on analyzer data from the last run.
        """
        self.portfolio.analyzer.create_full_tear_sheet()

    cpdef object get_orders_report(self):
        """
        Return an orders report dataframe.

        :return: pd.DataFrame.
        """
        return self._report_provider.get_orders_report(self._exec_client.get_orders_all())

    cpdef object get_order_fills_report(self):
        """
        Return an order fills report dataframe.
        
        :return: pd.DataFrame.
        """
        return self._report_provider.get_order_fills_report(self._exec_client.get_orders_all())

    cpdef object get_positions_report(self):
        """
        Return a positions report dataframe.

        :return: pd.DataFrame.
        """
        return self._report_provider.get_positions_report(self.portfolio.get_positions_all())
