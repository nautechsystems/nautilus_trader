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
from nautilus_trader.model.events cimport AccountEvent, OrderEvent, PositionOpened, PositionModified, PositionClosed
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.identifiers cimport StrategyId, OrderId, PositionId
from nautilus_trader.model.position cimport Position
from nautilus_trader.common.logger cimport Logger, LoggerAdapter
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.guid cimport LiveGuidFactory
from nautilus_trader.common.execution cimport ExecutionClient
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
        self._exec_client = None          # Initialized when registered with execution client
        self._position_book = {}          # type: Dict[PositionId, Position]
        self._order_p_index = {}          # type: Dict[OrderId, PositionId]
        self._registered_strategies = []  # type: List[StrategyId]
        self._positions_active = {}       # type: Dict[StrategyId, Dict[PositionId, Position]]
        self._positions_closed = {}       # type: Dict[StrategyId, Dict[PositionId, Position]]
        self._account_capital = Money.zero()
        self._account_initialized = False

        self.position_opened_events = []  # type: List[PositionOpened]
        self.position_closed_events = []  # type: List[PositionClosed]

        self.analyzer = PerformanceAnalyzer()

    cpdef list registered_strategies(self):
        """
        Return a list of strategy identifiers registered with the portfolio.

        :return: List[GUID].
        """
        return self._registered_strategies.copy()

    cpdef list registered_order_ids(self):
        """
        Return a list of order identifiers registered with the portfolio.
        
        :return: List[OrderId].
        """
        return list(self._order_p_index.keys())

    cpdef list registered_position_ids(self):
        """
        Return a list of position identifiers registered with the portfolio.
        
        :return: List[PositionId].
        """
        return list(self._order_p_index.values())

    cpdef Position get_position_for_order(self, OrderId order_id):
        """
        Return the position associated with the given order identifier.
        
        :param order_id: The order identifier.
        :return: Position (if found).
        :raises ValueError: If the position is not found.
        """
        Condition.is_in(order_id, self._order_p_index, 'order_id', 'order_p_index')

        cdef PositionId position_id = self._order_p_index[order_id]
        return self._position_book[position_id]

    cpdef Position get_position(self, PositionId position_id):
        """
        Return the position associated with the given position identifier.
        
        :param position_id: The position identifier.
        :return: Position (if found).
        :raises ValueError: If the position is not found.
        """
        Condition.is_in(position_id, self._position_book, 'position_id', 'position_book')

        return self._position_book[position_id]

    cpdef dict get_positions_all(self):
        """
        Return a dictionary of all positions held by the portfolio.
        
        :return: Dict[PositionId, Position].
        """
        return self._position_book.copy()

    cpdef dict get_positions_active_all(self):
        """
        Return a dictionary of all active positions held by the portfolio.
        
        :return: Dict[PositionId, Position].
        """
        return self._positions_active.copy()

    cpdef dict get_positions_closed_all(self):
        """
        Return a dictionary of all closed positions held by the portfolio.
        
        :return: Dict[PositionId, Position].
        """
        return self._positions_closed.copy()

    cpdef dict get_positions(self, StrategyId strategy_id):
        """
        Return a list of all positions associated with the given strategy identifier.
        
        :param strategy_id: The strategy identifier associated with the positions.
        :return: Dict[PositionId, Position].
        :raises ValueError: If the strategy identifier is not registered with the portfolio.
        """
        Condition.is_in(strategy_id, self._positions_active, 'strategy_id', 'positions_active')
        Condition.is_in(strategy_id, self._positions_closed, 'strategy_id', 'positions_closed')

        return {**self._positions_active[strategy_id], **self._positions_closed[strategy_id]}  # type: Dict[PositionId, Position]

    cpdef dict get_positions_active(self, StrategyId strategy_id):
        """
        Return a list of all active positions associated with the given strategy identifier.
        
        :param strategy_id: The strategy identifier associated with the positions.
        :return: Dict[PositionId, Position].
        :raises ValueError: If the strategy identifier is not registered with the portfolio.
        """
        Condition.is_in(strategy_id, self._positions_active, 'strategy_id', 'positions_active')

        return self._positions_active[strategy_id].copy()

    cpdef dict get_positions_closed(self, StrategyId strategy_id):
        """
        Return a list of all active positions associated with the given strategy identifier.
        
        :param strategy_id: The strategy identifier associated with the positions.
        :return: Dict[PositionId, Position].
        :raises ValueError: If the strategy identifier is not registered with the portfolio.
        """
        Condition.is_in(strategy_id, self._positions_closed, 'strategy_id', 'positions_closed')

        return self._positions_closed[strategy_id].copy()

    cpdef bint is_position_exists(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier exists.

        :param position_id: The position identifier.
        :return: True if the position exists, else False.
        """
        return position_id in self._position_book

    cpdef bint is_position_active(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier exists
        and is entered (active).

        :param position_id: The position identifier.
        :return: True if the position exists and is exited, else False.
        """
        return position_id in self._position_book and not self._position_book[position_id].is_flat

    cpdef bint is_position_closed(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier exists
        and is exited (closed).

        :param position_id: The position identifier.
        :return: True if the position does not exist or is closed, else False.
        """
        return position_id in self._position_book and self._position_book[position_id].is_exited

    cpdef bint is_position_for_order(self, OrderId order_id):
        """
        Return a value indicating whether there is a position associated with the given
        order identifier.
        
        :param order_id: The order identifier.
        :return: True if an associated position exists, else False.
        """
        return order_id in self._order_p_index and self._order_p_index[order_id] in self._position_book

    cpdef bint is_strategy_flat(self, StrategyId strategy_id):
        """
        Return a value indicating whether the strategy given identifier is flat 
        (all associated positions FLAT).
        
        :param strategy_id: The strategy identifier.
        :return: True if the strategy is flat, else False.
        :raises ValueError: If the strategy identifier is not registered with the portfolio.
        """
        Condition.is_in(strategy_id, self._positions_active, 'strategy_id', 'positions_active')

        return not self._positions_active[strategy_id]

    cpdef bint is_flat(self):
        """
        Return a value indicating whether the entire portfolio is flat.
        
        :return: True if the portfolio is flat, else False.
        """
        for strategy_id in self._registered_strategies:
            if not self.is_strategy_flat(strategy_id):
                return False  # Therefore the portfolio is not flat
        return True

    cpdef int positions_count(self):
        """
        Return the total count of active and closed positions.
        
        :return: int.
        """
        cdef int positions_total_count = 0

        positions_total_count += self.positions_active_count()
        positions_total_count += self.positions_closed_count()

        return positions_total_count

    cpdef int positions_active_count(self):
        """
        Return the count of active positions held by the portfolio.
        
        :return: int.
        """
        cdef int active_positions = 0

        for positions_list in self._positions_active.values():
            active_positions += len(positions_list)

        return active_positions

    cpdef int positions_closed_count(self):
        """
        Return the count of closed positions held by the portfolio.
        
        :return: int.
        """
        cdef int closed_count = 0

        for positions_list in self._positions_closed.values():
            closed_count += len(positions_list)

        return closed_count

    cpdef void register_execution_client(self, ExecutionClient client):
        """
        Register the given execution client with the portfolio to receive position events.
        
        :param client: The client to register.
        :raises ValueError: If the client is None.
        """
        Condition.not_none(client, 'client')

        self._exec_client = client
        self._log.debug("Registered execution client.")

    cpdef void register_strategy(self, TradeStrategy strategy):
        """
        Register the given strategy identifier with the portfolio.
        
        :param strategy: The strategy to register.
        :raises ValueError: If the strategy is already registered with the portfolio.
        """
        Condition.true(strategy.id not in self._registered_strategies, 'strategy_id not in self._registered_strategies')
        Condition.not_in(strategy.id, self._positions_active, 'strategy_id', 'active_positions')
        Condition.not_in(strategy.id, self._positions_closed, 'strategy_id', 'closed_positions')

        self._registered_strategies.append(strategy.id)
        self._positions_active[strategy.id] = {}  # type: Dict[PositionId, Position]
        self._positions_closed[strategy.id] = {}  # type: Dict[PositionId, Position]
        self._log.debug(f"Registered {strategy}.")

    cpdef void deregister_strategy(self, TradeStrategy strategy):
        """
        Deregister the given strategy with the portfolio.
        
        :param strategy: The strategy to deregister.
        :raises ValueError: If the strategy is not registered with the portfolio.
        """
        Condition.true(strategy.id in self._registered_strategies, 'strategy_id in self._registered_strategies')
        Condition.is_in(strategy.id, self._positions_active, 'strategy_id', 'active_positions')
        Condition.is_in(strategy.id, self._positions_closed, 'strategy_id', 'closed_positions')

        self._registered_strategies.remove(strategy.id)
        del self._positions_active[strategy.id]
        del self._positions_closed[strategy.id]
        self._log.debug(f"De-registered {strategy}.")

    cpdef void register_order(self, OrderId order_id, PositionId position_id):
        """
        Register the given order identifier with the given position identifier.
        
        :param order_id: The order identifier to register.
        :param position_id: The position identifier to register.
        :raises ValueError: If the order is already registered with the portfolio.
        """
        Condition.not_in(order_id, self._order_p_index, 'order_id', 'order_position_index')

        self._order_p_index[order_id] = position_id

    cpdef void handle_order_fill(self, OrderEvent event, StrategyId strategy_id):
        """
        Handle the order fill event associated with the given strategy identifier.
        
        :param event: The event to handle.
        :param strategy_id: The strategy identifier.
        :raises ValueError: If the events order identifier is not registered with the portfolio.
        :raises ValueError: If the strategy identifier is not registered with the portfolio.
        """
        Condition.is_in(event.order_id, self._order_p_index, 'event.order_id', 'order_position_index')
        Condition.true(strategy_id in self._registered_strategies, 'strategy_id in registered_strategies')

        cdef PositionId position_id = self._order_p_index[event.order_id]
        cdef Position position

        # Position does not exist yet
        if position_id not in self._position_book:
            # Create position
            position = Position(
                event.symbol,
                position_id,
                event.execution_time)
            position.apply(event)

            # Add position to position book
            self._position_book[position_id] = position

            # Add position to active positions
            self._positions_active[strategy_id][position_id] = position
            self._log.debug(f"Added {position} to active positions.")
            self._position_opened(position, strategy_id)

        # Position exists
        else:
            position = self._position_book[position_id]
            position.apply(event)

            if position.is_exited:
                # Move to closed positions
                if position_id in self._positions_active[strategy_id]:
                    self._positions_closed[strategy_id][position_id] = position
                    del self._positions_active[strategy_id][position_id]
                    self._log.debug(f"Moved {position} to closed positions.")
                    self._position_closed(position, strategy_id)
            else:
                # Check for overfill
                if position_id in self._positions_closed[strategy_id]:
                    self._positions_active[strategy_id][position_id] = position
                    del self._positions_closed[strategy_id][position_id]
                    self._log.warning(f"Moved {position} BACK to active positions due overfill.")
                    self._position_opened(position, strategy_id)
                self._position_modified(position, strategy_id)

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

    cpdef void check_residuals(self):
        """
        Check for any residual objects and log warnings if any are found.
        """
        for positions in self._positions_active.values():
            for position in positions.values():
                self._log.warning(f"Residual position {position}")

    cpdef void reset(self):
        """
        Reset the portfolio by returning all stateful internal values to their initial value.
        """
        self._log.info(f"Resetting...")
        self._position_book = {}                      # type: Dict[PositionId, Position]
        self._order_p_index = {}                      # type: Dict[OrderId, PositionId]

        # Reset all active positions
        for strategy_id in self._positions_active.keys():
            self._positions_active[strategy_id] = {}  # type: Dict[PositionId, Position]

        # Reset all closed positions
        for strategy_id in self._positions_closed.keys():
            self._positions_closed[strategy_id] = {}  # type: Dict[PositionId, Position]

        self._account_capital = Money.zero()
        self._account_initialized = False
        self.position_opened_events = []              # type: List[PositionOpened]
        self.position_closed_events = []              # type: List[PositionClosed]

        self.analyzer = PerformanceAnalyzer()
        self._log.info("Reset.")

    cdef void _position_opened(self, Position position, StrategyId strategy_id):
        cdef PositionOpened event = PositionOpened(
            position,
            strategy_id,
            self._guid_factory.generate(),
            self._clock.time_now())

        self.position_opened_events.append(event)
        self._exec_client.handle_event(event)

    cdef void _position_modified(self, Position position, StrategyId strategy_id):
        cdef PositionModified event = PositionModified(
            position,
            strategy_id,
            self._guid_factory.generate(),
            self._clock.time_now())

        self._exec_client.handle_event(event)

    cdef void _position_closed(self, Position position, StrategyId strategy_id):
        cdef datetime time_now = self._clock.time_now()
        cdef PositionClosed event = PositionClosed(
            position,
            strategy_id,
            self._guid_factory.generate(),
            time_now)

        self.position_closed_events.append(event)
        self.analyzer.add_return(time_now, position.return_realized)
        self._exec_client.handle_event(event)
