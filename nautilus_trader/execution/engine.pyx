# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

"""
The `ExecutionEngine` is the central component of the entire execution stack.

Its primary responsibility is to orchestrate interactions between the individual
`ExecutionClient` instances, and the rest of the platform. This includes
sending commands to and receiving events from particular venue endpoints.
Beneath it sits the `ExecutionCache` layer which presents a read-only facade
for consumers.

The engine employs a simple fan-in fan-out messaging pattern to receive events
from the `ExecutionClient` instances, and sending those to the registered
handlers, namely `TradingStrategy` instances.

Alternative implementations can be written on top which just need to override
the engines `execute` and `process` methods.
"""

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.generators cimport PositionIdGenerator
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport EVT
from nautilus_trader.common.logging cimport LogColour
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.fsm cimport InvalidStateTrigger
from nautilus_trader.core.time cimport unix_time
from nautilus_trader.execution.cache cimport ExecutionCache
from nautilus_trader.execution.client cimport ExecutionClient
from nautilus_trader.execution.database cimport ExecutionDatabase
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport AmendOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.events cimport Event
from nautilus_trader.model.events cimport OrderCancelReject
from nautilus_trader.model.events cimport OrderDenied
from nautilus_trader.model.events cimport OrderEvent
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport OrderInvalid
from nautilus_trader.model.events cimport PositionClosed
from nautilus_trader.model.events cimport PositionEvent
from nautilus_trader.model.events cimport PositionChanged
from nautilus_trader.model.events cimport PositionOpened
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order cimport Order
from nautilus_trader.trading.account cimport Account
from nautilus_trader.trading.portfolio cimport Portfolio
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class ExecutionEngine(Component):
    """
    Provides a high-performance execution engine for the management of many
    `ExecutionClient` instances, and the asynchronous ingest and distribution of
    trading commands and events.
    """

    def __init__(
        self,
        ExecutionDatabase database not None,
        Portfolio portfolio not None,
        Clock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the `ExecutionEngine` class.

        Parameters
        ----------
        database : ExecutionDatabase
            The execution database for the engine.
        portfolio : Portfolio
            The portfolio for the engine.
        clock : Clock
            The clock for the engine.
        logger : Logger
            The logger for the engine.
        config : dict[str, object], optional
            The configuration options.

        """
        if config is None:
            config = {}
        super().__init__(clock, logger, name="ExecEngine")

        self._clients = {}     # type: dict[Venue, ExecutionClient]
        self._strategies = {}  # type: dict[StrategyId, TradingStrategy]
        self._pos_id_generator = PositionIdGenerator(
            id_tag_trader=database.trader_id.tag,
            clock=clock,
        )
        self._portfolio = portfolio

        self.trader_id = database.trader_id
        self.cache = ExecutionCache(database, logger)

        # Counters
        self.command_count = 0
        self.event_count = 0

    @property
    def registered_venues(self):
        """
        The trading venues registered with the execution engine.

        Returns
        -------
        list[Venue]

        """
        return sorted(list(self._clients.keys()))

    @property
    def registered_strategies(self):
        """
        The strategy identifiers registered with the execution engine.

        Returns
        -------
        list[StrategyId]

        """
        return sorted(list(self._strategies.keys()))

    cpdef bint is_portfolio_equal(self, Portfolio portfolio) except *:
        """
        Return a value indicating whether the given portfolio is the same object
        as the portfolio wired to the execution engine.

        Parameters
        ----------
        portfolio : Portfolio
            The portfolio to check.

        Returns
        -------
        bool
            True if same object, else False.

        """
        return portfolio == self._portfolio

    cpdef bint check_connected(self) except *:
        """
        Check all of the engines clients are connected.

        Returns
        -------
        bool
            True if all clients connected, else False.

        """
        cdef ExecutionClient client
        for client in self._clients.values():
            if not client.is_connected:
                return False
        return True

    cpdef bint check_disconnected(self) except *:
        """
        Check all of the engines clients are disconnected.

        Returns
        -------
        bool
            True if all clients disconnected, else False.

        """
        cdef ExecutionClient client
        for client in self._clients.values():
            if client.is_connected:
                return False
        return True

# -- REGISTRATION ----------------------------------------------------------------------------------

    cpdef void register_client(self, ExecutionClient client) except *:
        """
        Register the given execution client with the execution engine.

        Parameters
        ----------
        client : ExecutionClient
            The execution client to register.

        Raises
        ------
        ValueError
            If client is already registered with the execution engine.

        """
        Condition.not_none(client, "client")
        Condition.not_in(client.venue, self._clients, "client.venue", "self._clients")

        self._clients[client.venue] = client
        self._log.info(f"Registered {client}.")

    cpdef void register_strategy(self, TradingStrategy strategy) except *:
        """
        Register the given strategy with the execution engine.

        Parameters
        ----------
        strategy : TradingStrategy
            The strategy to register.

        Raises
        ------
        ValueError
            If strategy is already registered with the execution engine.

        """
        Condition.not_none(strategy, "strategy")
        Condition.not_in(strategy.id, self._strategies, "strategy.id", "registered_strategies")

        strategy.register_execution_engine(self)
        strategy.register_portfolio(self._portfolio)
        self._strategies[strategy.id] = strategy
        self._log.info(f"Registered {strategy}.")

    cpdef void deregister_client(self, ExecutionClient client) except *:
        """
        Deregister the given execution client from the execution engine.

        Parameters
        ----------
        client : ExecutionClient
            The execution client to deregister.

        Raises
        ------
        ValueError
            If client is not registered with the execution engine.

        """
        Condition.not_none(client, "client")
        Condition.is_in(client.venue, self._clients, "client.venue", "self._clients")

        del self._clients[client.venue]
        self._log.info(f"Deregistered {client}.")

    cpdef void deregister_strategy(self, TradingStrategy strategy) except *:
        """
        Deregister the given strategy with the execution engine.

        Parameters
        ----------
        strategy : TradingStrategy
            The strategy to deregister.

        Raises
        ------
        ValueError
            If strategy is not registered with the execution engine.

        """
        Condition.not_none(strategy, "strategy")
        Condition.is_in(strategy.id, self._strategies, "strategy.id", "registered_strategies")

        del self._strategies[strategy.id]
        self._log.info(f"Deregistered {strategy}.")

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void _on_start(self) except *:
        pass  # Optionally override in subclass

    cpdef void _on_stop(self) except *:
        pass  # Optionally override in subclass

# -- ACTION IMPLEMENTATIONS ------------------------------------------------------------------------

    cpdef void _start(self) except *:
        cdef ExecutionClient client
        for client in self._clients.values():
            client.connect()

        # Initialize portfolio
        self._portfolio.initialize_orders(set(self.cache.orders_working()))
        self._portfolio.initialize_positions(set(self.cache.positions_open()))

        self._on_start()

    cpdef void _stop(self) except *:
        cdef ExecutionClient client
        for client in self._clients.values():
            client.disconnect()

        self._on_stop()

    cpdef void _reset(self) except *:
        for client in self._clients.values():
            client.reset()

        self.cache.reset()
        self._pos_id_generator.reset()

        self.command_count = 0
        self.event_count = 0

    cpdef void _dispose(self) except *:
        cdef ExecutionClient client
        for client in self._clients.values():
            client.dispose()

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void load_cache(self) except *:
        """
        Load the cache up from the execution database.
        """
        cdef double ts = unix_time()

        self.cache.cache_accounts()
        self.cache.cache_orders()
        self.cache.cache_positions()
        self.cache.build_index()
        self.cache.check_integrity()
        self._set_position_id_counts()

        self._log.info(f"Loaded cache in {unix_time() - ts:.3f}s.")

        # Update portfolio
        for account in self.cache.accounts():
            self._portfolio.register_account(account)

    cpdef void check_integrity(self) except *:
        """
        Check integrity of data within the cache and clients.
        """
        self.cache.check_integrity()

        # For each client check integrity
        # TODO: Implement

    cpdef void execute(self, VenueCommand command) except *:
        """
        Execute the given command.

        Parameters
        ----------
        command : VenueCommand
            The command to execute.

        """
        Condition.not_none(command, "command")

        self._execute_command(command)

    cpdef void process(self, Event event) except *:
        """
        Process the given event.

        Parameters
        ----------
        event : Event
            The event to process.

        """
        Condition.not_none(event, "event")

        self._handle_event(event)

    cpdef void check_residuals(self) except *:
        """
        Check for residual working orders or open positions.
        """
        self.cache.check_residuals()

    cpdef void flush_db(self) except *:
        """
        Flush the execution database which permanently removes all persisted data.

        Warnings
        --------
        Permanent data loss.

        """
        self.cache.flush_db()

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cdef inline void _execute_command(self, VenueCommand command) except *:
        self._log.debug(f"{RECV}{CMD} {command}.")
        self.command_count += 1

        cdef ExecutionClient client = self._clients.get(command.venue)
        if client is None:
            self._log.error(f"Cannot handle command, "
                            f"no client registered for {command.venue}, {command}.")
            return  # No client to handle command

        if isinstance(command, SubmitOrder):
            self._handle_submit_order(client, command)
        elif isinstance(command, SubmitBracketOrder):
            self._handle_submit_bracket_order(client, command)
        elif isinstance(command, AmendOrder):
            self._handle_amend_order(client, command)
        elif isinstance(command, CancelOrder):
            self._handle_cancel_order(client, command)
        else:
            self._log.error(f"Cannot handle unrecognized command, {command}.")

    cdef inline void _handle_submit_order(self, ExecutionClient client, SubmitOrder command) except *:
        # Validate command
        if self.cache.order_exists(command.order.cl_ord_id):
            self._invalidate_order(
                command.order.cl_ord_id,
                f"{repr(command.order.cl_ord_id)} already exists",
            )
            return  # Invalid command

        # Cache order
        self.cache.add_order(command.order, command.position_id)

        if command.position_id.not_null() and not self.cache.position_exists(command.position_id):
            self._invalidate_order(
                command.order.cl_ord_id,
                f"{repr(PositionId)} does not exist",
            )
            return  # Invalid command

        # Submit order
        client.submit_order(command)

    cdef inline void _handle_submit_bracket_order(self, ExecutionClient client, SubmitBracketOrder command) except *:
        # Validate command
        if self.cache.order_exists(command.bracket_order.entry.cl_ord_id):
            self._invalidate_order(
                command.bracket_order.entry.cl_ord_id,
                f"{repr(command.bracket_order.entry.cl_ord_id)} already exists",
            )
            self._invalidate_order(
                command.bracket_order.stop_loss.cl_ord_id,
                f"parent {repr(command.bracket_order.entry.cl_ord_id)} already exists",
            )
            if command.bracket_order.take_profit is not None:
                self._invalidate_order(
                    command.bracket_order.take_profit.cl_ord_id,
                    f"parent {repr(command.bracket_order.entry.cl_ord_id)} already exists",
                )
            return  # Invalid command
        if self.cache.order_exists(command.bracket_order.stop_loss.cl_ord_id):
            self._invalidate_order(
                command.bracket_order.entry.cl_ord_id,
                f"OCO {repr(command.bracket_order.stop_loss.cl_ord_id)} already exists",
            )
            self._invalidate_order(
                command.bracket_order.stop_loss.cl_ord_id,
                f"{repr(command.bracket_order.stop_loss.cl_ord_id)} already exists",
            )
            if command.bracket_order.take_profit is not None:
                self._invalidate_order(
                    command.bracket_order.take_profit.cl_ord_id,
                    f"OCO {repr(command.bracket_order.stop_loss.cl_ord_id)} already exists",
                )
            return  # Invalid command
        if command.bracket_order.take_profit is not None and self.cache.order_exists(command.bracket_order.take_profit.cl_ord_id):
            self._invalidate_order(
                command.bracket_order.entry.cl_ord_id,
                f"OCO {repr(command.bracket_order.take_profit.cl_ord_id)} already exists",
            )
            self._invalidate_order(
                command.bracket_order.stop_loss.cl_ord_id,
                f"OCO {repr(command.bracket_order.take_profit.cl_ord_id)} already exists",
            )
            self._invalidate_order(
                command.bracket_order.take_profit.cl_ord_id,
                f"{repr(command.bracket_order.take_profit.cl_ord_id)} already exists",
            )
            return  # Invalid command

        # Cache all orders
        self.cache.add_order(command.bracket_order.entry, PositionId.null_c())
        self.cache.add_order(command.bracket_order.stop_loss, PositionId.null_c())
        if command.bracket_order.take_profit is not None:
            self.cache.add_order(command.bracket_order.take_profit, PositionId.null_c())

        # Submit bracket order
        client.submit_bracket_order(command)

    cdef inline void _handle_amend_order(self, ExecutionClient client, AmendOrder command) except *:
        # Validate command
        if not self.cache.is_order_working(command.cl_ord_id):
            self._log.warning(f"Cannot amend order,  "
                              f"{repr(command.cl_ord_id)} already completed.")
            return  # Invalid command

        client.amend_order(command)

    cdef inline void _handle_cancel_order(self, ExecutionClient client, CancelOrder command) except *:
        # Validate command
        if self.cache.is_order_completed(command.cl_ord_id):
            self._log.warning(f"Cannot cancel order, "
                              f"{repr(command.cl_ord_id)} already completed.")
            return  # Invalid command

        client.cancel_order(command)

    cdef inline void _invalidate_order(self, ClientOrderId cl_ord_id, str reason) except *:
        # Generate event
        cdef OrderInvalid invalid = OrderInvalid(
            cl_ord_id,
            reason,
            self._uuid_factory.generate(),
            self._clock.utc_now_c(),
        )

        self._handle_event(invalid)

    cdef inline void _deny_order(self, ClientOrderId cl_ord_id, str reason) except *:
        # Generate event
        cdef OrderDenied denied = OrderDenied(
            cl_ord_id,
            reason,
            self._uuid_factory.generate(),
            self._clock.utc_now_c(),
        )

        self._handle_event(denied)

# -- EVENT HANDLERS --------------------------------------------------------------------------------

    cdef inline void _handle_event(self, Event event) except *:
        self._log.debug(f"{RECV}{EVT} {event}.")
        self.event_count += 1

        if isinstance(event, OrderEvent):
            self._handle_order_event(event)
        elif isinstance(event, PositionEvent):
            self._handle_position_event(event)
        elif isinstance(event, AccountState):
            self._handle_account_event(event)
        else:
            self._log.error(f"Cannot handle unrecognized event, {event}.")

    cdef inline void _handle_account_event(self, AccountState event) except *:
        cdef Account account = self.cache.account(event.account_id)
        if account is None:
            # Generate account
            account = Account(event)
            self.cache.add_account(account)
            self._portfolio.register_account(account)
        else:
            account.apply_c(event)
            self.cache.update_account(account)

    cdef inline void _handle_position_event(self, PositionEvent event) except *:
        self._portfolio.update_position(event)
        self._send_to_strategy(event, event.position.strategy_id)

    cdef inline void _handle_order_event(self, OrderEvent event) except *:
        if isinstance(event, OrderCancelReject):
            self._handle_order_cancel_reject(event)
            return  # Event will be sent to strategy

        # Fetch Order from cache
        cdef ClientOrderId cl_ord_id = event.cl_ord_id
        cdef Order order = self.cache.order(event.cl_ord_id)
        cdef str event_str
        if order is None:
            self._log.warning(f"{repr(event.cl_ord_id)} was not found in cache "
                              f"for {repr(event.order_id)} to apply {event}.")

            # Search cache for ClientOrderId matching the OrderId
            cl_ord_id = self.cache.cl_ord_id(event.order_id)
            if cl_ord_id is None:
                self._log.error(f"Cannot apply event to any order, "
                                f"no matching ClientOrderId found in cache.")
                return  # Cannot process event further

            # Search cache for Order matching the found ClientOrderId
            order = self.cache.order(cl_ord_id)
            if order is None:
                self._log.error(f"Cannot apply event to any order, "
                                f"order for {repr(cl_ord_id)} not found in cache.")
                return  # Cannot process event further

            # Set the correct ClientOrderId for the event
            event.cl_ord_id = cl_ord_id
            self._log.warning(f"{repr(cl_ord_id)} was found in cache and "
                              f"applying event to order with {repr(order.id)}.")

        try:
            order.apply_c(event)
        except InvalidStateTrigger as ex:
            self._log.exception(ex)
            return  # Not re-raising to avoid crashing engine
        except ValueError as ex:
            # Catches order identifier validations
            self._log.exception(ex)
            return  # Not re-raising to avoid crashing engine

        self.cache.update_order(order)

        # Update portfolio
        if order.is_passive_c() and (order.is_working_c() or order.is_completed_c()):
            self._portfolio.update_order(order)

        if isinstance(event, OrderFilled):
            self._handle_order_fill(event)
            return  # Event will be sent to strategy

        self._send_to_strategy(event, self.cache.strategy_id_for_order(cl_ord_id))

    cdef inline void _handle_order_cancel_reject(self, OrderCancelReject event) except *:
        cdef StrategyId strategy_id = self.cache.strategy_id_for_order(event.cl_ord_id)
        if strategy_id is None:
            self._log.error(f"Cannot process event "
                            f"(strategy identifier not found), {event}.")
            return  # Cannot process event further

        self._send_to_strategy(event, strategy_id)

    cdef inline void _handle_order_fill(self, OrderFilled fill) except *:
        # Get PositionId corresponding to fill
        cdef PositionId position_id = self.cache.position_id(fill.cl_ord_id)
        # --- position_id could be None here (position not opened yet) ---

        # Get StrategyId corresponding to fill
        cdef StrategyId strategy_id = self.cache.strategy_id_for_order(fill.cl_ord_id)
        if strategy_id is None and fill.position_id.not_null():
            strategy_id = self.cache.strategy_id_for_position(fill.position_id)
        if strategy_id is None:
            self._log.error(f"Cannot process event (StrategyId for "
                            f"{repr(fill.cl_ord_id)} or"
                            f"{repr(fill.position_id)} not found), {fill}.")
            return  # Cannot process event further

        if fill.position_id.is_null():  # Exchange not assigning position_ids
            self._fill_with_no_position_id(position_id, fill, strategy_id)
        else:
            self._fill_with_assigned_position_id(position_id, fill, strategy_id)

    cdef inline void _fill_with_no_position_id(
        self,
        PositionId position_id,
        OrderFilled fill,
        StrategyId strategy_id,
    ) except *:
        if position_id is None:  # No position yet
            # Generate identifier and assign
            fill.position_id = self._pos_id_generator.generate(strategy_id)

            # Create new position
            self._open_position(fill)
        else:  # Position exists
            fill.position_id = position_id
            fill.strategy_id = strategy_id
            self._update_position(fill)

    cdef inline void _fill_with_assigned_position_id(
        self,
        PositionId position_id,
        OrderFilled fill,
        StrategyId strategy_id,
    ) except *:
        fill.strategy_id = strategy_id
        if position_id is None:  # No position
            self._open_position(fill)
        else:
            self._update_position(fill)

    cdef inline void _open_position(self, OrderFilled fill) except *:
        cdef Position position = Position(fill)
        self.cache.add_position(position)

        self._send_to_strategy(fill, fill.strategy_id)
        self.process(self._pos_opened_event(position, fill))

    cdef inline void _update_position(self, OrderFilled fill) except *:
        cdef Position position = self.cache.position(fill.position_id)
        if position is None:
            self._log.error(f"Cannot update position for "
                            f"{repr(fill.position_id)} "
                            f"(no position found in cache).")
            return  # Cannot process event further

        # Check for flip
        if fill.order_side != position.entry and fill.fill_qty > position.quantity:
            self._flip_position(position, fill)
            return  # Handled in flip

        position.apply_c(fill)
        self.cache.update_position(position)

        cdef PositionEvent position_event
        if position.is_closed_c():
            position_event = self._pos_closed_event(position, fill)
        else:
            position_event = self._pos_changed_event(position, fill)

        self._send_to_strategy(fill, fill.strategy_id)
        self.process(position_event)

    cdef inline void _flip_position(self, Position position, OrderFilled fill) except *:
        cdef Quantity difference
        if position.side == PositionSide.LONG:
            difference = Quantity(fill.fill_qty - position.quantity)
        else:  # position.side == PositionSide.SHORT:
            difference = Quantity(abs(position.quantity - fill.fill_qty))

        # Split commission between two positions
        fill_percent1 = position.quantity / fill.fill_qty
        fill_percent2 = 1 - fill_percent1  # Subtract from an integer to return a Decimal

        # Split fill to close original position
        cdef OrderFilled fill_split1 = OrderFilled(
            fill.account_id,
            fill.cl_ord_id,
            fill.order_id,
            fill.execution_id,
            fill.position_id,
            fill.strategy_id,
            fill.symbol,
            fill.order_side,
            position.quantity,                       # Fill original position quantity remaining
            Quantity(fill.cum_qty - difference),     # Adjust cumulative qty by difference
            Quantity(fill.leaves_qty + difference),  # Adjust leaves qty by difference
            fill.fill_price,
            fill.currency,
            fill.is_inverse,
            Money(fill.commission * fill_percent1, fill.commission.currency),
            fill.liquidity_side,
            fill.execution_time,
            fill.id,
            fill.timestamp,
        )

        # Close original position
        position.apply_c(fill_split1)
        self.cache.update_position(position)

        self._send_to_strategy(fill, fill.strategy_id)
        self.process(self._pos_closed_event(position, fill))

        # Generate position identifier for flipped position
        cdef PositionId position_id_flip = self._pos_id_generator.generate(
            strategy_id=fill.strategy_id,
            flipped=True,
        )

        # Split fill to open flipped position
        cdef OrderFilled fill_split2 = OrderFilled(
            fill.account_id,
            ClientOrderId(f"{fill.cl_ord_id.value}F"),
            fill.order_id,
            fill.execution_id,
            position_id_flip,
            fill.strategy_id,
            fill.symbol,
            fill.order_side,
            difference,  # Fill difference from original as above
            fill.cum_qty,
            fill.leaves_qty,
            fill.fill_price,
            fill.currency,
            fill.is_inverse,
            Money(fill.commission * fill_percent2, fill.commission.currency),
            fill.liquidity_side,
            fill.execution_time,
            self._uuid_factory.generate(),  # New event identifier
            fill.timestamp,
        )

        cdef Position position_flip = Position(fill_split2)
        self.cache.add_position(position_flip)
        self.process(self._pos_opened_event(position_flip, fill_split2))

    cdef inline PositionOpened _pos_opened_event(self, Position position, OrderFilled event):
        return PositionOpened(
            position,
            event,
            self._uuid_factory.generate(),
            event.timestamp,
        )

    cdef inline PositionChanged _pos_changed_event(self, Position position, OrderFilled event):
        return PositionChanged(
            position,
            event,
            self._uuid_factory.generate(),
            event.timestamp,
        )

    cdef inline PositionClosed _pos_closed_event(self, Position position, OrderFilled event):
        return PositionClosed(
            position,
            event,
            self._uuid_factory.generate(),
            event.timestamp,
        )

    cdef inline void _send_to_strategy(self, Event event, StrategyId strategy_id) except *:
        if strategy_id is None:
            self._log.error(f"Cannot send event to strategy "
                            f"({repr(strategy_id)} not found), {event}.")
            return  # Cannot send to strategy

        cdef TradingStrategy strategy = self._strategies.get(strategy_id)
        if strategy is None:
            self._log.error(f"Cannot send event to strategy "
                            f"({repr(strategy_id)} not registered), {event}.")
            return  # Cannot send to strategy

        strategy.handle_event_c(event)

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef inline void _set_position_id_counts(self) except *:
        # For the internal position identifier generator
        cdef list positions = self.cache.positions()

        # Count positions per symbol
        cdef dict counts = {}  # type: dict[StrategyId, int]
        cdef int count
        cdef Position position
        for position in positions:
            count = counts.get(position.strategy_id, 0)
            count += 1
            # noinspection PyUnresolvedReferences
            counts[position.strategy_id] = count

        # Reset position identifier generator
        self._pos_id_generator.reset()

        # Set counts
        cdef StrategyId strategy_id
        for strategy_id, count in counts.items():
            self._pos_id_generator.set_count(strategy_id, count)
            self._log.info(f"Set PositionId count for {repr(strategy_id)} to {count}.", LogColour.BLUE)
