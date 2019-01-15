#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from pandas import DataFrame
from typing import Set, List, Dict, Callable

from inv_trader.core.decimal cimport Decimal
from inv_trader.core.precondition cimport Precondition
from inv_trader.enums.resolution cimport Resolution
from inv_trader.common.clock cimport Clock, TestClock
from inv_trader.common.logger cimport Logger, LoggerAdapter
from inv_trader.model.account cimport Account
from inv_trader.model.objects cimport Symbol
from inv_trader.model.order cimport Order
from inv_trader.model.events cimport Event, OrderEvent, AccountEvent, OrderCancelReject
from inv_trader.model.identifiers cimport GUID, OrderId
from inv_trader.common.execution cimport ExecutionClient


cdef class BacktestExecClient(ExecutionClient):
    """
    Provides an execution client for the BacktestEngine.
    """

    def __init__(self,
                 dict tick_data: Dict[Symbol, DataFrame],
                 dict bar_data_bid: Dict[Symbol, Dict[Resolution, DataFrame]],
                 dict bar_data_ask: Dict[Symbol, Dict[Resolution, DataFrame]],
                 TestClock clock,
                 Logger logger):
        """
        Initializes a new instance of the BacktestDataClient class.

        :param bar_data_bid: The historical bid market data needed for the backtest.
        :param bar_data_ask: The historical ask market data needed for the backtest.
        :param clock: The clock for the component.
        :param logger: The logger for the component.
        """
        Precondition.not_none(clock, 'clock')
        Precondition.not_none(logger, 'logger')

        super().__init__(clock, logger)

        self.tick_data = tick_data
        self.bar_data_bid = bar_data_bid
        self.bar_data_ask = bar_data_ask
        self.iteration = 0

    cpdef void connect(self):
        """
        Connect to the execution service.
        """
        # Do nothing
        pass

    cpdef void disconnect(self):
        """
        Disconnect from the execution service.
        """
        # Do nothing
        pass

    cpdef void collateral_inquiry(self):
        """
        Send a collateral inquiry command to the execution service.
        """
        # Do nothing
        pass

    cpdef void submit_order(self, Order order, GUID strategy_id):
        """
        Send a submit order request to the execution service.
        """
        # Do nothing
        pass

    cpdef void cancel_order(self, Order order, str cancel_reason):
        """
        Send a cancel order request to the execution service.
        """
        # Do nothing
        pass

    cpdef void modify_order(self, Order order, Decimal new_price):
        """
        Send a modify order request to the execution service.
        """
        # Do nothing
        pass

    cpdef void iterate(self, datetime time):
        """
        Iterate the data client one time step.
        """
        pass
