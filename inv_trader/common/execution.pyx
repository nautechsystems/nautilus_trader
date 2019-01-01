#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False


from decimal import Decimal
from typing import Dict, Callable
from uuid import UUID

from inv_trader.core.precondition cimport Precondition
from inv_trader.core.logger import Logger, LoggerAdapter
from inv_trader.model.account cimport Account
from inv_trader.model.order cimport Order
from inv_trader.model.events cimport Event, OrderEvent, AccountEvent, OrderCancelReject
from inv_trader.model.identifiers cimport GUID, OrderId
from inv_trader.strategy import TradeStrategy

cdef str UTF8 = 'utf-8'


cdef class ExecutionClient(object):
    """
    The abstract base class for all execution clients.
    """

    def __init__(self, logger: Logger=None):
        """
        Initializes a new instance of the ExecutionClient class.

        :param logger: The logging adapter for the component.
        """
        Precondition.type_or_none(logger, Logger, 'logger')

        if logger is None:
            self.log = LoggerAdapter(f"ExecClient")
        else:
            self.log = LoggerAdapter(f"ExecClient", logger)
        self.log.info("Initialized.")
        self.account = Account()
        self._registered_strategies = {}  # type: Dict[UUID, Callable]
        self._order_index = {}            # type: Dict[OrderId, UUID]

    cpdef void register_strategy(self, strategy: TradeStrategy):
        """
        Register the given strategy with the execution client.

        :raises ValueError: If the strategy is already registered (must have a unique UUID id).
        """
        Precondition.type(strategy, TradeStrategy, 'strategy')

        if strategy.id in self._registered_strategies:
            raise ValueError(
                "Cannot register strategy (The strategy must have a unique UUID id).")

        self._registered_strategies[strategy.id] = strategy._update_events
        strategy._register_execution_client(self)

        self.log.info(f"Registered strategy {strategy} with the execution client.")

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

    cpdef void submit_order(
            self,
            order: Order,
            GUID strategy_id):
        """
        Send a submit order request to the execution service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the execution client.")

    cpdef void cancel_order(
            self, Order order,
            str cancel_reason):
        """
        Send a cancel order request to the execution service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the execution client.")

    cpdef void modify_order(
            self,
            Order order,
            new_price: Decimal):
        """
        Send a modify order request to the execution service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the execution client.")

    cpdef void _register_order(
            self,
            Order order,
            GUID strategy_id):
        """
        Register the given order with the execution client.

        :param order: The order to register.
        :param strategy_id: The strategy id to register with the order.
        """
        if order.id in self._order_index:
            raise ValueError(f"The order does not have a unique id.")

        self._order_index[order.id] = strategy_id

    cpdef void _on_event(self, Event event):
        """
        Handle events received from the execution service.
        """
        self.log.debug(f"Received {event}")

        if isinstance(event, OrderEvent):
            order_id = event.order_id
            if order_id not in self._order_index.keys():
                self.log.warning(
                    f"The given event order id {order_id} was not contained in the order index.")
                return

            strategy_id = self._order_index[order_id]
            self._registered_strategies[strategy_id](event)

            if isinstance(event, OrderCancelReject):
                self.log.warning(f"{event}")

        elif isinstance(event, AccountEvent):
            self.account.apply(event)
