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
from inv_trader.core.decimal cimport Decimal
from inv_trader.common.clock cimport Clock, LiveClock
from inv_trader.common.logger cimport Logger, LoggerAdapter
from inv_trader.model.account cimport Account
from inv_trader.model.order cimport Order
from inv_trader.model.events cimport Event, OrderEvent, AccountEvent, OrderRejected, OrderCancelReject
from inv_trader.model.identifiers cimport GUID, OrderId
from inv_trader.strategy cimport TradeStrategy

cdef str UTF8 = 'utf-8'


cdef class ExecutionClient:
    """
    The abstract base class for all execution clients.
    """

    def __init__(self,
                 Clock clock=LiveClock(),
                 Logger logger=None):
        """
        Initializes a new instance of the ExecutionClient class.

        :param clock: The internal clock.
        :param logger: The internal logger.
        """
        self._clock = clock
        if logger is None:
            self._log = LoggerAdapter(f"ExecClient")
        else:
            self._log = LoggerAdapter(f"ExecClient", logger)
        self._log.info("Initialized.")
        self.account = Account()
        self._registered_strategies = {}  # type: Dict[GUID, TradeStrategy]
        self._order_index = {}            # type: Dict[OrderId, GUID]

    cpdef datetime time_now(self):
        """
        :return: The current time of the internal clock. 
        """
        return self._clock.time_now()

    cpdef void register_strategy(self, TradeStrategy strategy):
        """
        Register the given strategy with the execution client.

        :raises ValueError: If the strategy is already registered (must have a unique UUID id).
        """
        if strategy.id in self._registered_strategies:
            raise ValueError(
                "Cannot register strategy (The strategy must have a unique UUID id).")

        self._registered_strategies[strategy.id] = strategy
        strategy._register_execution_client(self)  # Access to protected member ok here

        self._log.info(f"Registered strategy {strategy}.")

    cpdef void connect(self):
        """
        Connect to the execution service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the execution client.")

    cpdef void disconnect(self):
        """
        Disconnect from the execution service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the execution client.")

    cpdef void collateral_inquiry(self):
        """
        Send a collateral inquiry command to the execution service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the execution client.")

    cpdef void submit_order(self, Order order, GUID strategy_id):
        """
        Send a submit order request to the execution service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the execution client.")

    cpdef void cancel_order(self, Order order, str cancel_reason):
        """
        Send a cancel order request to the execution service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the execution client.")

    cpdef void modify_order(self, Order order, Decimal new_price):
        """
        Send a modify order request to the execution service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the execution client.")

    cdef void _register_order(self, Order order, GUID strategy_id):
        """
        Register the given order with the execution client.

        :param order: The order to register.
        :param strategy_id: The strategy id to register with the order.
        """
        Precondition.true(order.id not in self._order_index, 'order.id NOT in self._order_index')

        self._order_index[order.id] = strategy_id

    cdef void _on_event(self, Event event):
        """
        Handle events received from the execution service.
        """
        self._log.debug(f"Received {event}")

        if isinstance(event, OrderEvent):
            if isinstance(event, OrderRejected):
                self._log.warning(f"{event} {event.rejected_reason}")
            elif isinstance(event, OrderCancelReject):
                self._log.warning(f"{event} {event.cancel_reject_reason} {event.cancel_reject_response}")

            if event.order_id not in self._order_index:
                self._log.warning(
                    f"The given event order id {event.order_id} was not contained in the order index.")
                return

            strategy_id = self._order_index[event.order_id]
            self._registered_strategies[strategy_id]._update_events(event)  # Access to protected member ok here

        elif isinstance(event, AccountEvent):
            self.account.apply(event)
