#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

import uuid

from pandas import DataFrame
from typing import Set, List, Dict, Callable

from inv_trader.core.decimal cimport Decimal
from inv_trader.core.precondition cimport Precondition
from inv_trader.enums.brokerage cimport Broker
from inv_trader.enums.currency_code cimport CurrencyCode
from inv_trader.enums.resolution cimport Resolution
from inv_trader.model.identifiers cimport AccountNumber
from inv_trader.common.clock cimport Clock, TestClock
from inv_trader.common.logger cimport Logger, LoggerAdapter
from inv_trader.model.account cimport Account
from inv_trader.model.objects import Money
from inv_trader.model.objects cimport Symbol, Instrument
from inv_trader.model.order cimport Order
from inv_trader.model.events cimport Event, OrderEvent, AccountEvent, OrderCancelReject
from inv_trader.model.identifiers cimport GUID, OrderId
from inv_trader.common.execution cimport ExecutionClient


cdef class BacktestExecClient(ExecutionClient):
    """
    Provides an execution client for the BacktestEngine.
    """

    def __init__(self,
                 list instruments: List[Instrument],
                 dict tick_data: Dict[Symbol, DataFrame],
                 dict bar_data_bid: Dict[Symbol, Dict[Resolution, DataFrame]],
                 dict bar_data_ask: Dict[Symbol, Dict[Resolution, DataFrame]],
                 Decimal starting_capital,
                 TestClock clock,
                 Logger logger):
        """
        Initializes a new instance of the BacktestDataClient class.

        :param instruments: The instruments needed for the backtest.
        :param tick_data: The historical tick market data needed for the backtest.
        :param bar_data_bid: The historical bid market data needed for the backtest.
        :param bar_data_ask: The historical ask market data needed for the backtest.
        :param starting_capital: The starting capital for the backtest account (> 0).
        :param clock: The clock for the component.
        :param logger: The logger for the component.
        """
        Precondition.positive(starting_capital, 'starting_capital')
        Precondition.not_none(clock, 'clock')
        Precondition.not_none(logger, 'logger')

        super().__init__(clock, logger)

        # Convert instruments list to dictionary indexed by symbol
        cdef dict instruments_dict = {}  # type: Dict[Symbol, Instrument]
        for instrument in instruments:
            instruments_dict[instrument.symbol] = instrument
        self.instruments = instruments_dict
        self.tick_data = tick_data
        self.bar_data_bid = bar_data_bid
        self.bar_data_ask = bar_data_ask
        self.iteration = 0
        self.account_cash_start_day = starting_capital
        self.account_cash_activity_day = Decimal(0, 2)
        self.working_orders = dict()  # type: Dict[OrderId, Order]

        cdef AccountEvent initial_starting = AccountEvent(self.account.id,
                                                          Broker.SIMULATED,
                                                          AccountNumber('9999'),
                                                          CurrencyCode.USD,
                                                          starting_capital,
                                                          starting_capital,
                                                          Money.zero(),
                                                          Money.zero(),
                                                          Money.zero(),
                                                          Decimal(0),
                                                          'NONE',
                                                          GUID(uuid.uuid4()),
                                                          self._clock.time_now())

        self.account.apply(initial_starting)

    cpdef void connect(self):
        """
        Connect to the execution service.
        """
        self._log.info("Connected.")

    cpdef void disconnect(self):
        """
        Disconnect from the execution service.
        """
        self._log.info("Disconnected.")

    cpdef void collateral_inquiry(self):
        """
        Send a collateral inquiry command to the execution service.
        """
        cdef AccountEvent event = AccountEvent(self.account.id,
                                               self.account.broker,
                                               self.account.account_number,
                                               self.account.currency,
                                               self.account.cash_balance,
                                               self.account_cash_start_day,
                                               self.account_cash_activity_day,
                                               self.account.margin_used_liquidation,
                                               self.account.margin_used_maintenance,
                                               self.account.margin_ratio,
                                               self.account.margin_call_status,
                                               GUID(uuid.uuid4()),
                                               self._clock.time_now())
        self._on_event(event)

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
        for order in self.working_orders:
            pass
