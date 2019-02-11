#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime
from typing import Dict
from queue import Queue

from inv_trader.core.precondition cimport Precondition
from inv_trader.common.clock cimport Clock
from inv_trader.common.guid cimport GuidFactory
from inv_trader.common.logger cimport Logger, LoggerAdapter
from inv_trader.common.account cimport Account
from inv_trader.model.order cimport Order
from inv_trader.model.events cimport Event, OrderEvent, AccountEvent, OrderModified
from inv_trader.model.events cimport OrderRejected, OrderCancelled, OrderCancelReject, OrderFilled, OrderPartiallyFilled
from inv_trader.model.identifiers cimport GUID, OrderId, PositionId
from inv_trader.commands cimport Command, CollateralInquiry
from inv_trader.commands cimport SubmitOrder, SubmitAtomicOrder, ModifyOrder, CancelOrder
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
        self._account = account
        self._portfolio = portfolio
        self._queue = Queue()
        self._registered_strategies = {}  # type: Dict[GUID, TradeStrategy]
        self._order_book = {}             # type: Dict[OrderId, Order]
        self._order_strategy_index = {}   # type: Dict[OrderId, GUID]
        self._orders_active = {}          # type: Dict[GUID, Dict[OrderId, Order]]
        self._orders_completed = {}       # type: Dict[GUID, Dict[OrderId, Order]]

        self._log.info(f"Initialized.")

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

    cpdef void register_strategy(self, TradeStrategy strategy):
        """
        Register the given strategy with the execution client.

        :raises ValueError: If the strategy is already registered (must have a unique id).
        """
        Precondition.not_in(strategy.id, self._registered_strategies, 'strategy', 'registered_strategies')
        Precondition.not_in(strategy.id, self._orders_active, 'strategy', 'orders_active')
        Precondition.not_in(strategy.id, self._orders_completed, 'strategy', 'orders_completed')

        self._registered_strategies[strategy.id] = strategy
        self._orders_active[strategy.id] = {}     # type: Dict[OrderId, Order]
        self._orders_completed[strategy.id] = {}  # type: Dict[OrderId, Order]

        self._portfolio.register_strategy(strategy.id)
        strategy.register_execution_client(self)

        self._log.info(f"Registered {strategy} with id {strategy.id}.")

    cpdef void execute_command(self, Command command):
        """
        Execute the given command by putting it on the internal queue for processing.
        
        :param command: The command to execute.
        """
        self._queue.put(command)

    cpdef void handle_event(self, Event event):
        """
        Handle the given event by putting it on the internal queue for processing.
        
        :param event: The event to handle
        """
        self._queue.put(event)

    cpdef Order get_order(self, OrderId order_id):
        """
        Return the order with the given identifier (if found).
        
        :return: Order.
        :raises ValueError: If the order is not found.
        """
        Precondition.is_in(order_id, self._order_book, 'order_id', 'order_book')

        return self._order_book[order_id]

    cpdef dict get_orders_all(self):
        """
        Return a dictionary of all orders in the execution clients order book.
        
        :return: Dict[OrderId, Order].
        """
        return self._order_book.copy()

    cpdef dict get_orders_active_all(self):
        """
        Return a dictionary of all active orders in the execution clients order book.
        
        :return: Dict[OrderId, Order].
        """
        return self._orders_active.copy()

    cpdef dict get_orders_completed_all(self):
        """
        Return a dictionary of all completed orders in the execution clients order book.
        
        :return: Dict[OrderId, Order].
        """
        return self._orders_completed.copy()

    cpdef dict get_orders(self, GUID strategy_id):
        """
        Return a dictionary of all orders associated with the strategy identifier.
        
        :param strategy_id: The strategy identifier associated with the orders.
        :return: Dict[OrderId, Order].
        """
        Precondition.is_in(strategy_id, self._orders_active, 'strategy_id', 'orders_active')
        Precondition.is_in(strategy_id, self._orders_completed, 'strategy_id', 'orders_completed')

        cpdef dict orders = {**self._orders_active[strategy_id], **self._orders_completed[strategy_id]}
        return orders  # type: Dict[OrderId, Order]

    cpdef dict get_orders_active(self, GUID strategy_id):
        """
        Return a dictionary of all active orders associated with the strategy identifier.
        
        :param strategy_id: The strategy identifier associated with the orders.
        :return: Dict[OrderId, Order].
        """
        Precondition.is_in(strategy_id, self._orders_active, 'strategy_id', 'orders_active')

        return self._orders_active[strategy_id].copy()

    cpdef dict get_orders_completed(self, GUID strategy_id):
        """
        Return a list of all completed orders associated with the strategy identifier.
        
        :param strategy_id: The strategy identifier associated with the orders.
        :return: Dict[OrderId, Order].
        """
        Precondition.is_in(strategy_id, self._orders_completed, 'strategy_id', 'orders_completed')

        return self._orders_completed[strategy_id].copy()

    cdef void _execute_command(self, Command command):
        """
        Execute the given command received from a strategy.
        
        :param command: The command to execute.
        """
        self._log.debug(f"Received {command}")

        if isinstance(command, CollateralInquiry):
            self._collateral_inquiry(command)
        elif isinstance(command, SubmitOrder):
            self._register_order(command.order, command.position_id, command.strategy_id)
            self._submit_order(command)
        elif isinstance(command, SubmitAtomicOrder):
            self._register_order(command.atomic_order.entry, command.position_id, command.strategy_id)
            self._register_order(command.atomic_order.stop_loss, command.position_id, command.strategy_id)
            if command.has_profit_target:
                self._register_order(command.atomic_order.profit_target, command.position_id, command.strategy_id)
            self._submit_atomic_order(command)
        elif isinstance(command, ModifyOrder):
            self._modify_order(command)
        elif isinstance(command, CancelOrder):
            self._cancel_order(command)

    cdef void _handle_event(self, Event event):
        """
        Handle the given event received from the execution service.
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

            # Active order
            if order.is_active:
                if order.id not in self._orders_active[strategy_id]:
                    self._orders_active[strategy_id][order.id] = order

            # Completed order
            if order.is_complete:
                if order.id not in self._orders_completed[strategy_id]:
                    self._orders_completed[strategy_id][order.id] = order
                    if order.id in self._orders_active[strategy_id]:
                        del self._orders_active[strategy_id][order.id]

            if isinstance(event, OrderFilled) or isinstance(event, OrderPartiallyFilled):
                self._portfolio.handle_event(event, strategy_id)
            elif isinstance(event, OrderModified):
                self._log.info(f"{event} price to {event.modified_price}")
            elif isinstance(event, OrderCancelled):
                self._log.info(str(event))
            # Warning Events
            elif isinstance(event, OrderRejected):
                self._log.warning(f"{event} {event.rejected_reason}")
            elif isinstance(event, OrderCancelReject):
                self._log.warning(f"{event} {event.cancel_reject_reason} {event.cancel_reject_response}")

            # Send event to strategy
            self._registered_strategies[strategy_id].handle_event(event)

        elif isinstance(event, AccountEvent):
            self._account.apply(event)

    cdef void _register_order(self, Order order, PositionId position_id, GUID strategy_id):
        """
        Register the given order with the execution client.

        :param order: The order to register.
        :param position_id: The order identifier to associate with the order.
        :param strategy_id: The strategy identifier to associate with the order.
        :raises ValueError: If the order.id is already in the order_index.
        """
        Precondition.not_in(order.id, self._order_book, 'order.id', 'order_book')
        Precondition.not_in(order.id, self._order_strategy_index, 'order.id', 'order_index')

        self._order_book[order.id] = order
        self._order_strategy_index[order.id] = strategy_id
        self._portfolio.register_order(order.id, position_id)
        self._log.debug(f"Registered {order.id} with {position_id} for strategy with id {strategy_id}.")

    cdef void _collateral_inquiry(self, CollateralInquiry command):
        """
        Send a collateral inquiry command to the execution service.
        
        :param command: The command to send.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef void _submit_order(self, SubmitOrder command):
        """
        Send a submit order command to the execution service.
        
        :param command: The command to send.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef void _submit_atomic_order(self, SubmitAtomicOrder command):
        """
        Send a submit atomic order command to the execution service.
        
        :param command: The command to send.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef void _modify_order(self, ModifyOrder command):
        """
        Send a modify order command to the execution service.
        
        :param command: The command to send.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef void _cancel_order(self, CancelOrder command):
        """
        Send a cancel order command to the execution service.
        
        :param command: The command to send.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")
