# -------------------------------------------------------------------------------------------------
# <copyright file="portfolio.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from typing import List, Dict
from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.events cimport (
    AccountEvent,
    OrderEvent,
    OrderFilled,
    OrderPartiallyFilled,
    PositionOpened,
    PositionModified,
    PositionClosed)
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.identifiers cimport StrategyId, OrderId, PositionId
from nautilus_trader.model.position cimport Position
from nautilus_trader.common.logger cimport Logger, LoggerAdapter
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.guid cimport LiveGuidFactory
from nautilus_trader.trade.performance cimport PerformanceAnalyzer


cdef class Portfolio:
    """
    Provides a trading portfolio of positions.
    """

    def __init__(self,
                 Clock clock=LiveClock(),
                 GuidFactory guid_factory=LiveGuidFactory(),
                 Logger logger=None):
        """
        Initializes a new instance of the Portfolio class.

        :param clock: The clock for the component.
        :param guid_factory: The guid factory for the component.
        :param logger: The logger for the component.
        """
        self._clock = clock
        self._guid_factory = guid_factory
        self._log = LoggerAdapter(self.__class__.__name__, logger)
        self._exec_engine = None          # Initialized when registered with execution engine

        self._account_capital = Money.zero()
        self._account_initialized = False

        self.analyzer = PerformanceAnalyzer()

    cpdef void handle_transaction(self, AccountEvent event):
        """
        Handle the transaction associated with the given account event.

        :param event: The event to handle.
        """
        # Account data initialization
        if not self._account_initialized:
            self.analyzer.set_starting_capital(event.cash_balance, event.currency)
            self._account_capital = event.cash_balance
            self._account_initialized = True
            return

        if self._account_capital == event.cash_balance:
            return  # No transaction to handle

        # Calculate transaction data
        cdef Money pnl = event.cash_balance - self._account_capital
        self._account_capital = event.cash_balance

        self.analyzer.add_transaction(event.timestamp, self._account_capital, pnl)

    cpdef void reset(self):
        """
        Reset the portfolio by returning all stateful internal values to their initial value.
        """
        self._log.info(f"Resetting...")

        self._account_capital = Money.zero()
        self._account_initialized = False

        self.analyzer = PerformanceAnalyzer()
        self._log.info("Reset.")
