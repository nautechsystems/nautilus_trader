#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="trader.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime
from typing import List

from inv_trader.core.precondition cimport Precondition
from inv_trader.common.account cimport Account
from inv_trader.common.clock cimport LiveClock
from inv_trader.common.logger cimport Logger, LoggerAdapter, LiveLogger
from inv_trader.common.data cimport DataClient
from inv_trader.common.execution cimport ExecutionClient
from inv_trader.portfolio.portfolio cimport Portfolio
from inv_trader.strategy cimport TradeStrategy


cdef class Trader:
    """
    Provides a trader for managing a portfolio of trade strategies.
    """

    def __init__(self,
                 str order_id_tag,
                 list strategies,
                 DataClient data_client,
                 ExecutionClient exec_client,
                 Account account,
                 Portfolio portfolio,
                 Clock clock=LiveClock(),
                 Logger logger=LiveLogger()):
        """
        Initializes a new instance of the Trader class.

        :param order_id_tag: The unique order identifier tag for the trader (unique at fund level).
        :param strategies: The initial list of strategies to manage (cannot be empty).
        :param logger: The logger for the trader (can be None).
        :raise ValueError: If the label is an invalid string.
        :raise ValueError: If the strategies list is empty.
        :raise ValueError: If the strategies list contains a type other than TradeStrategy.
        :raise ValueError: If the data client is None.
        :raise ValueError: If the exec client is None.
        :raise ValueError: If the clock is None.
        """
        Precondition.valid_string(order_id_tag, 'order_id_tag')
        Precondition.not_empty(strategies, 'strategies')
        Precondition.list_type(strategies, TradeStrategy, 'strategies')
        Precondition.not_none(data_client, 'data_client')
        Precondition.not_none(exec_client, 'exec_client')
        Precondition.not_none(clock, 'clock')

        self._clock = clock
        self.id = TraderId(self.__class__.__name__ + '-' + order_id_tag)
        self.order_id_tag = ValidString(order_id_tag)
        if logger is None:
            self._log = LoggerAdapter(f"{self.id.value}")
        else:
            self._log = LoggerAdapter(f"{self.id.value}", logger)

        self.is_running = False
        self.started_datetimes = []  # type: List[datetime]
        self.stopped_datetimes = []  # type: List[datetime]

        self._data_client = data_client
        self._exec_client = exec_client
        self.account = account
        self.portfolio = portfolio
        self.portfolio.register_execution_client(self._exec_client)
        self.strategies = strategies
        self._initialize_strategies()

    cpdef void start(self):
        """
        Start the trader.
        """
        self.started_datetimes.append(self._clock.time_now())

        self._log.info("Starting...")
        self._data_client.connect()
        self._exec_client.connect()

        self._data_client.update_all_instruments()

        for strategy in self.strategies:
            strategy.start()

        self.is_running = True
        self._log.info("Running...")

    cpdef void stop(self):
        """
        Stop the trader.
        """
        self.stopped_datetimes.append(self._clock.time_now())

        self._log.info("Stopping...")
        for strategy in self.strategies:
            strategy.stop()

        self.is_running = False
        self._log.info("Stopped.")
        self._exec_client.check_residuals()
        self.portfolio.check_residuals()

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

    cpdef void change_strategies(self, list strategies: List[TradeStrategy]):
        """
        Change strategies with the given list of trade strategies.
        
        :param strategies: The list of strategies to load into the trader.
        :raises ValueError: If the strategies list contains a type other than TradeStrategy.
        """
        Precondition.list_type(strategies, TradeStrategy, 'strategies')

        self.strategies = strategies
        self._initialize_strategies()

    cpdef void reset(self):
        """
        Reset the trader.
        """
        if self.is_running:
            self._log.warning(f"Cannot reset a running Trader...")
            return

        self._log.info("Resetting...")
        for strategy in self.strategies:
            strategy.reset()

        self.portfolio.reset()
        self._log.info("Reset.")

    cpdef void dispose(self):
        """
        Dispose of the trader.
        """
        self._log.info("Disposing...")
        for strategy in self.strategies:
            strategy.dispose()

        self._data_client.disconnect()
        self._exec_client.disconnect()
        self._log.info("Disposed.")

    cpdef dict strategy_status(self):
        """
        Return a dictionary containing the traders strategy status.
        The key is the strategy identifier.
        The value is a bool which is True if the strategy is running else False.
        
        :return: Dict[StrategyId, bool].
        """
        cdef status_list = {}
        for strategy in self.strategies:
            if strategy.is_running:
                status_list[strategy.id] = True
            else:
                status_list[strategy.id] = False

        return status_list

    cdef void _initialize_strategies(self):
        for strategy in self.strategies:
            # TODO: Check for matching ids
            strategy.register_trader_id(self.id, self.order_id_tag)
            self._data_client.register_strategy(strategy)
            self._exec_client.register_strategy(strategy)
            self._log.info(f"Initialized {strategy}.")
