#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

from cpython.datetime cimport datetime
from typing import Dict

from inv_trader.core.precondition cimport Precondition
from inv_trader.common.clock cimport Clock
from inv_trader.common.guid cimport GuidFactory
from inv_trader.common.logger cimport Logger, LoggerAdapter
from inv_trader.model.account cimport Account
from inv_trader.model.objects cimport Price
from inv_trader.model.order cimport Order
from inv_trader.model.events cimport Event, OrderEvent, AccountEvent
from inv_trader.model.events cimport OrderRejected, OrderCancelReject, OrderFilled, OrderPartiallyFilled
from inv_trader.model.identifiers cimport GUID, OrderId
from inv_trader.strategy cimport TradeStrategy
from inv_trader.portfolio.portfolio cimport Portfolio

cdef str UTF8 = 'utf-8'


cdef class ExecutionClient:
    """
    The abstract base class for all execution clients.
    """

    def __init__(self,
                 Account account,
                 Portfolio portfolio,
                 Clock clock,
                 GuidFactory guid_factory,
                 Logger logger):
        """
        Initializes a new instance of the ExecutionClient class.

        :param clock: The internal clock.
        :param guid_factory: The internal GUID factory.
        :param logger: The internal logger.
        """
        self._clock = clock
        self._guid_factory = guid_factory
        if logger is None:
            self._log = LoggerAdapter(f"ExecClient")
        else:
            self._log = LoggerAdapter(f"ExecClient", logger)
        self._log.info("Initialized.")
        self._account = account
        self._portfolio = portfolio
        self._registered_strategies = {}   # type: Dict[GUID, TradeStrategy]
        self._order_book = {}              # type: Dict[OrderId, Order]
        self._order_strategy_index = {}    # type: Dict[OrderId, GUID]

    cpdef datetime time_now(self):
        """
        :return: The current time of the execution client. 
        """
        return self._clock.time_now()

    cpdef Account get_account(self):
        """
        :return: The account associated with the execution client. 
        """
        return self._account

    cpdef Portfolio get_portfolio(self):
        """
        :return: The portfolio associated with the execution client. 
        """
        return self._portfolio

    cpdef void register_strategy(self, TradeStrategy strategy):
        """
        Register the given strategy with the execution client.

        :raises ValueError: If the strategy is already registered (must have a unique id).
        """
        Precondition.not_in(strategy.id, self._registered_strategies, 'strategy', 'registered_strategies')

        self._registered_strategies[strategy.id] = strategy
        self._portfolio._register_strategy(self)   # Access to protected member ok here
        strategy._register_execution_client(self)  # Access to protected member ok here

        self._log.info(f"Registered strategy {strategy} with id {strategy.id}.")

    cpdef void connect(self):
        """
        Connect to the execution service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void disconnect(self):
        """
        Disconnect from the execution service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void collateral_inquiry(self):
        """
        Send a collateral inquiry command to the execution service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void submit_order(self, Order order, GUID strategy_id):
        """
        Send a submit order request to the execution service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void cancel_order(self, Order order, str cancel_reason):
        """
        Send a cancel order request to the execution service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void modify_order(self, Order order, Price new_price):
        """
        Send a modify order request to the execution service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef void _register_order(self, Order order, GUID strategy_id):
        """
        Register the given order with the execution client.

        :param order: The order to register.
        :param strategy_id: The strategy id to register with the order.
        :raises ValueError: If the order.id is already in the order_index.
        """
        Precondition.not_in(order.id, self._order_strategy_index, 'order.id', 'order_index')

        self._order_book[order.id] = order
        self._order_strategy_index[order.id] = strategy_id

    cdef void _on_event(self, Event event):
        """
        Handle events received from the execution service.
        """
        self._log.debug(f"Received {event}")

        cdef Order order
        cdef GUID strategy_id

        # Order events
        if isinstance(event, OrderEvent):
            Precondition.is_in(event.order_id, self._order_book, 'order_id', 'order_book')
            Precondition.is_in(event.order_id, self._order_strategy_index, 'order_id', 'order_strategy_index')

            order = self._order_book[event.order_id]
            order.apply(event)

            strategy_id = self._order_strategy_index[event.order_id]

            # Position events
            if isinstance(event, OrderFilled) or isinstance(event, OrderPartiallyFilled):
                self._portfolio._on_event(event, strategy_id)  # Access to protected member ok here
            # Warning Events
            elif isinstance(event, OrderRejected):
                self._log.warning(f"{event} {event.rejected_reason}")
            elif isinstance(event, OrderCancelReject):
                self._log.warning(f"{event} {event.cancel_reject_reason} {event.cancel_reject_response}")

            self._registered_strategies[strategy_id]._update_events(event)  # Access to protected member ok here

        elif isinstance(event, AccountEvent):
            self.account.apply(event)
