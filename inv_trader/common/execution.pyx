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

from inv_trader.core.precondition cimport Precondition
from inv_trader.common.clock cimport Clock
from inv_trader.common.guid cimport GuidFactory
from inv_trader.common.logger cimport Logger, LoggerAdapter
from inv_trader.common.account cimport Account
from inv_trader.model.order cimport Order
from inv_trader.model.events cimport Event, OrderEvent, PositionEvent, AccountEvent, OrderModified
from inv_trader.model.events cimport OrderRejected, OrderCancelled, OrderCancelReject, OrderFilled, OrderPartiallyFilled
from inv_trader.model.identifiers cimport StrategyId, OrderId, PositionId
from inv_trader.commands cimport Command, CollateralInquiry
from inv_trader.commands cimport SubmitOrder, SubmitAtomicOrder, ModifyOrder, CancelOrder
from inv_trader.strategy cimport TradeStrategy
from inv_trader.portfolio.portfolio cimport Portfolio

cdef str UTF8 = 'utf-8'


cdef class ExecutionClient:
    """
    The base class for all execution clients.
    """

    def __init__(self,
                 Account account,
                 Portfolio portfolio,
                 Clock clock,
                 GuidFactory guid_factory,
                 Logger logger):
        """
        Initializes a new instance of the ExecutionClient class.

        :param account: The account for the execution client.
        :param portfolio: The portfolio for the execution client.
        :param clock: The clock for the component.
        :param guid_factory: The GUID factory for the component.
        :param logger: The logger for the component.
        """
        self._clock = clock
        self._guid_factory = guid_factory
        if logger is None:
            self._log = LoggerAdapter(f"ExecClient")
        else:
            self._log = LoggerAdapter(f"ExecClient", logger)
        self._account = account
        self._portfolio = portfolio
        self._registered_strategies = {}  # type: Dict[StrategyId, TradeStrategy]
        self._order_book = {}             # type: Dict[OrderId, Order]
        self._order_strategy_index = {}   # type: Dict[OrderId, StrategyId]
        self._orders_active = {}          # type: Dict[StrategyId, Dict[OrderId, Order]]
        self._orders_completed = {}       # type: Dict[StrategyId, Dict[OrderId, Order]]
        self.event_count = 0

        self._log.info(f"Initialized.")

    cpdef datetime time_now(self):
        """
        Return the current time of the execution client.
         
        :return: datetime. 
        """
        return self._clock.time_now()

    cpdef Account get_account(self):
        """
        Return The account associated with the execution client.
        
        :return: Account. 
        """
        return self._account

    cpdef Portfolio get_portfolio(self):
        """
        Return the portfolio associated with the execution client.
        
        :return: Portfolio. 
        """
        return self._portfolio

    cpdef void connect(self):
        """
        Connect to the execution service.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void disconnect(self):
        """
        Disconnect from the execution service.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void check_residuals(self):
        """
        Check for any residual objects and log warnings if any are found.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void execute_command(self, Command command):
        """
        Execute the given command with the execution client.
        
        :param command: The command to execute.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void handle_event(self, Event event):
        """
        Handle the given event with the execution client.
        
        :param event: The event to handle
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void register_strategy(self, TradeStrategy strategy):
        """
        Register the given strategy with the execution client.

        :param strategy: The strategy to register.
        :raises ValueError: If the strategy is already registered with the execution client.
        """
        Precondition.not_in(strategy.id, self._registered_strategies, 'strategy', 'registered_strategies')
        Precondition.not_in(strategy.id, self._orders_active, 'strategy', 'orders_active')
        Precondition.not_in(strategy.id, self._orders_completed, 'strategy', 'orders_completed')

        self._registered_strategies[strategy.id] = strategy
        self._orders_active[strategy.id] = {}     # type: Dict[OrderId, Order]
        self._orders_completed[strategy.id] = {}  # type: Dict[OrderId, Order]

        self._portfolio.register_strategy(strategy)
        strategy.register_execution_client(self)

        self._log.debug(f"Registered {strategy}.")

    cpdef void deregister_strategy(self, TradeStrategy strategy):
        """
        Deregister the given strategy with the execution client.

        :param strategy: The strategy to deregister.
        :raises ValueError: If the strategy is not registered with the execution client.
        """
        Precondition.is_in(strategy.id, self._registered_strategies, 'strategy', 'registered_strategies')
        Precondition.is_in(strategy.id, self._orders_active, 'strategy', 'orders_active')
        Precondition.is_in(strategy.id, self._orders_completed, 'strategy', 'orders_completed')

        del self._registered_strategies[strategy.id]
        del self._orders_active[strategy.id]
        del self._orders_completed[strategy.id]

        self._portfolio.deregister_strategy(strategy)

        self._log.debug(f"Deregistered {strategy}.")

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

    cpdef dict get_orders(self, StrategyId strategy_id):
        """
        Return a dictionary of all orders associated with the strategy identifier.
        
        :param strategy_id: The strategy identifier associated with the orders.
        :return: Dict[OrderId, Order].
        :raises ValueError: If the strategy identifier is not registered with the execution client.
        """
        Precondition.is_in(strategy_id, self._orders_active, 'strategy_id', 'orders_active')
        Precondition.is_in(strategy_id, self._orders_completed, 'strategy_id', 'orders_completed')

        return {**self._orders_active[strategy_id], **self._orders_completed[strategy_id]}

    cpdef dict get_orders_active(self, StrategyId strategy_id):
        """
        Return a dictionary of all active orders associated with the strategy identifier.
        
        :param strategy_id: The strategy identifier associated with the orders.
        :return: Dict[OrderId, Order].
        :raises ValueError: If the strategy identifier is not registered with the execution client.
        """
        Precondition.is_in(strategy_id, self._orders_active, 'strategy_id', 'orders_active')

        return self._orders_active[strategy_id].copy()

    cpdef dict get_orders_completed(self, StrategyId strategy_id):
        """
        Return a list of all completed orders associated with the strategy identifier.
        
        :param strategy_id: The strategy identifier associated with the orders.
        :return: Dict[OrderId, Order].
        :raises ValueError: If the strategy identifier is not registered with the execution client.
        """
        Precondition.is_in(strategy_id, self._orders_completed, 'strategy_id', 'orders_completed')

        return self._orders_completed[strategy_id].copy()

    cpdef bint is_order_exists(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier exists.
        
        :param order_id: The order identifier to check.
        :return: True if the order exists, else False.
        """
        return order_id in self._order_book

    cpdef bint is_order_active(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier is active.
         
        :param order_id: The order identifier to check.
        :return: True if the order is active, else False.
        :raises ValueError: If the order is not found.
        """
        Precondition.is_in(order_id, self._order_book, 'order_id', 'order_book')

        return self._order_book[order_id].is_active

    cpdef bint is_order_complete(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier is complete.
         
        :param order_id: The order identifier to check.
        :return: True if the order is complete, else False.
        :raises ValueError: If the order is not found.
        """
        Precondition.is_in(order_id, self._order_book, 'order_id', 'order_book')

        return self._order_book[order_id].is_complete

    cdef void _execute_command(self, Command command):
        """
        Execute the given command received from a strategy.
        
        :param command: The command to execute.
        """
        self._log.debug(f"Received {command}")

        if isinstance(command, CollateralInquiry):
            self._collateral_inquiry(command)
        elif isinstance(command, SubmitOrder):
            self._register_order(command.order, command.strategy_id, command.position_id)
            self._submit_order(command)
        elif isinstance(command, SubmitAtomicOrder):
            self._register_order(command.atomic_order.entry, command.strategy_id, command.position_id)
            self._register_order(command.atomic_order.stop_loss, command.strategy_id, command.position_id)
            if command.atomic_order.has_take_profit:
                self._register_order(command.atomic_order.take_profit, command.strategy_id, command.position_id)
            self._submit_atomic_order(command)
        elif isinstance(command, ModifyOrder):
            self._modify_order(command)
        elif isinstance(command, CancelOrder):
            self._cancel_order(command)

    cdef void _handle_event(self, Event event):
        """
        Handle the given event received from the execution service.
 
        :param: event: The event to handle.
        """
        self.event_count += 1

        cdef Order order
        cdef StrategyId strategy_id

        # Order events
        if isinstance(event, OrderEvent):
            if event.order_id in self._order_book:
                order = self._order_book[event.order_id]
            else:
                self._log.error(f"order for {event.order_id} not found")
                return # Cannot apply event to an order

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

            # Send event to strategy
            self._registered_strategies[strategy_id].handle_event(event)

            if isinstance(event, (OrderFilled, OrderPartiallyFilled)):
                self._log.debug(f'{event}')
                self._portfolio.handle_order_fill(event, strategy_id)
            elif isinstance(event, OrderModified):
                self._log.debug(f"{event} price to {event.modified_price}")
            elif isinstance(event, OrderCancelled):
                self._log.debug(str(event))
            # Warning Events
            elif isinstance(event, (OrderRejected, OrderCancelReject)):
                self._log.debug(f'{event}')  # Also logged as warning by strategy
            else:
                self._log.debug(f'{event}')

        # Position events
        elif isinstance(event, PositionEvent):
            self._log.debug(f'{event}')
            # Send event to strategy
            self._registered_strategies[event.strategy_id].handle_event(event)

        # Account event
        elif isinstance(event, AccountEvent):
            self._log.debug(f'{event}')
            self._account.apply(event)
            self._portfolio.handle_transaction(event)

    cdef void _register_order(self, Order order, StrategyId strategy_id, PositionId position_id):
        """
        Register the given order with the execution client.

        :param order: The order to register.
        :param strategy_id: The strategy identifier to associate with the order.
        :param position_id: The order identifier to associate with the order.
        :raises ValueError: If the order identifier is already registered with the execution client.
        """
        Precondition.not_in(order.id, self._order_book, 'order.id', 'order_book')
        Precondition.not_in(order.id, self._order_strategy_index, 'order.id', 'order_index')

        self._order_book[order.id] = order
        self._order_strategy_index[order.id] = strategy_id
        self._portfolio.register_order(order.id, position_id)
        self._log.debug(f"Registered {order.id} for strategy {strategy_id} with {position_id}.")


# -- ABSTRACT METHODS ---------------------------------------------------------------------------- #

    cdef void _collateral_inquiry(self, CollateralInquiry command):
        """
        Send a collateral inquiry command to the execution service.
        
        :param command: The command to send.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef void _submit_order(self, SubmitOrder command):
        """
        Send a submit order command to the execution service.
        
        :param command: The command to send.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef void _submit_atomic_order(self, SubmitAtomicOrder command):
        """
        Send a submit atomic order command to the execution service.
        
        :param command: The command to send.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef void _modify_order(self, ModifyOrder command):
        """
        Send a modify order command to the execution service.
        
        :param command: The command to send.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef void _cancel_order(self, CancelOrder command):
        """
        Send a cancel order command to the execution service.
        
        :param command: The command to send.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef void _check_residuals(self):
        """
        Check for any residual active orders and log warnings if any are found.
        """
        for orders in self._orders_active.values():
            for order in orders.values():
                self._log.warning(f"Residual active {order}")

    cdef void _reset(self):
        """
        Reset the execution client by returning all stateful internal values to their initial value.
        """
        self._log.debug(f"Resetting...")
        self._order_book = {}                         # type: Dict[OrderId, Order]
        self._order_strategy_index = {}               # type: Dict[OrderId, StrategyId]
        self.event_count = 0

        # Reset all active orders
        for strategy_id in self._orders_active.keys():
            self._orders_active[strategy_id] = {}     # type: Dict[OrderId, Order]

        # Reset all completed orders
        for strategy_id in self._orders_completed.keys():
            self._orders_completed[strategy_id] = {}  # type: Dict[OrderId, Order]
