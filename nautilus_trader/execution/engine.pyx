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

The execution engines primary responsibility is to orchestrate interactions
between the `ExecutionClient` instances, and the rest of the platform. This
includes sending commands to, and receiving events from, the particular venue
endpoints via its registered execution clients.

Beneath it sits an `ExecutionCache` which presents a read-only facade for
consumers. The engine employs a simple fan-in fan-out messaging pattern to execute
`TradingCommand` messages, and process `AccountState` or `OrderEvent` type
messages.

Alternative implementations can be written on top of the generic engine - which
just need to override the `execute` and `process` methods.
"""

from libc.stdint cimport int64_t

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.generators cimport PositionIdGenerator
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport EVT
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.fsm cimport InvalidStateTrigger
from nautilus_trader.core.time cimport unix_timestamp_us
from nautilus_trader.execution.cache cimport ExecutionCache
from nautilus_trader.execution.client cimport ExecutionClient
from nautilus_trader.execution.database cimport ExecutionDatabase
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.commands cimport UpdateOrder
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.events cimport Event
from nautilus_trader.model.events cimport OrderCancelRejected
from nautilus_trader.model.events cimport OrderEvent
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport OrderInvalid
from nautilus_trader.model.events cimport OrderUpdateRejected
from nautilus_trader.model.events cimport PositionChanged
from nautilus_trader.model.events cimport PositionClosed
from nautilus_trader.model.events cimport PositionEvent
from nautilus_trader.model.events cimport PositionOpened
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order.base cimport Order
from nautilus_trader.model.order.bracket cimport BracketOrder
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

        self._clients = {}     # type: dict[str, ExecutionClient]
        self._strategies = {}  # type: dict[StrategyId, TradingStrategy]
        self._pos_id_generator = PositionIdGenerator(
            id_tag_trader=database.trader_id.tag,
            clock=clock,
        )
        self._portfolio = portfolio
        self._risk_engine = None

        self.trader_id = database.trader_id
        self.cache = ExecutionCache(database, logger)

        # Counters
        self.command_count = 0
        self.event_count = 0

    @property
    def registered_clients(self):
        """
        The execution clients registered with the engine.

        Returns
        -------
        list[str]

        """
        return sorted(list(self._clients.keys()))

    @property
    def registered_strategies(self):
        """
        The strategy identifiers registered with the engine.

        Returns
        -------
        list[StrategyId]

        """
        return sorted(list(self._strategies.keys()))

    cpdef int position_id_count(self, StrategyId strategy_id) except *:
        """
        The position identifier count for the given strategy identifier.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy identifier for the position count.

        Returns
        -------
        int

        """
        return self._pos_id_generator.get_count(strategy_id)

    cpdef bint check_portfolio_equal(self, Portfolio portfolio) except *:
        """
        Check whether the given portfolio is the same object as the portfolio
        wired to the execution engine.

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

    cpdef bint check_integrity(self) except *:
        """
        Check integrity of data within the cache and clients.

        Returns
        -------
        bool
            True if checks pass, else False.
        """
        return self.cache.check_integrity()

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

    cpdef bint check_residuals(self) except *:
        """
        Check for any residual active state and log warnings if found.

        Active state is considered working orders and open positions.

        Returns
        -------
        bool
            True if residuals exist, else False.

        """
        return self.cache.check_residuals()

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
        Condition.not_in(client.name, self._clients, "client.name", "self._clients")

        self._clients[client.name] = client
        self._log.info(f"Registered {client}.")

        if self._risk_engine is not None and client not in self._risk_engine.registered_clients:
            self._risk_engine.register_client(client)

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

    cpdef void register_risk_engine(self, RiskEngine engine) except *:
        """
        Register the given risk engine with the execution engine.

        Parameters
        ----------
        engine : RiskEngine
            The risk engine to register.

        """
        Condition.not_none(engine, "engine")

        self._risk_engine = engine
        self._log.info(f"Registered {engine}.")

        cdef list risk_registered = self._risk_engine.registered_clients

        cdef ExecutionClient client
        for venue, client in self._clients.items():
            if venue not in risk_registered:
                self._risk_engine.register_client(client)

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
        Condition.is_in(client.name, self._clients, "client.name", "self._clients")

        del self._clients[client.name]
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
        cdef int64_t ts = unix_timestamp_us()

        self.cache.cache_accounts()
        self.cache.cache_orders()
        self.cache.cache_positions()
        self.cache.build_index()
        self.cache.check_integrity()
        self._set_position_id_counts()

        self._log.info(f"Loaded cache in {(unix_timestamp_us() - ts)}μs.")

        # Update portfolio
        for account in self.cache.accounts():
            self._portfolio.register_account(account)

    cpdef void execute(self, TradingCommand command) except *:
        """
        Execute the given command.

        Parameters
        ----------
        command : TradingCommand
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

    cpdef void flush_db(self) except *:
        """
        Flush the execution database which permanently removes all persisted data.

        Warnings
        --------
        Permanent data loss.

        """
        self.cache.flush_db()

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef inline void _set_position_id_counts(self) except *:
        # For the internal position identifier generator
        cdef list positions = self.cache.positions()

        # Count positions per instrument_id
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
            self._log.info(f"Set PositionId count for {repr(strategy_id)} to {count}.")

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cdef inline void _execute_command(self, TradingCommand command) except *:
        self._log.debug(f"{RECV}{CMD} {command}.")
        self.command_count += 1

        cdef ExecutionClient client = self._clients.get(command.instrument_id.venue.first())
        if client is None:
            self._log.error(f"Cannot handle command: "
                            f"No client registered for {command.instrument_id.venue.first()}, {command}.")
            return  # No client to handle command

        if isinstance(command, SubmitOrder):
            self._handle_submit_order(client, command)
        elif isinstance(command, SubmitBracketOrder):
            self._handle_submit_bracket_order(client, command)
        elif isinstance(command, UpdateOrder):
            self._handle_update_order(client, command)
        elif isinstance(command, CancelOrder):
            self._handle_cancel_order(client, command)
        else:
            self._log.error(f"Cannot handle command: unrecognized {command}.")

    cdef inline void _handle_submit_order(self, ExecutionClient client, SubmitOrder command) except *:
        # Validate command
        if self.cache.order_exists(command.order.cl_ord_id):
            self._log.error(f"Cannot submit order: "
                            f"{repr(command.order.cl_ord_id)} already exists.")
            return  # Invalid command

        # Cache order
        self.cache.add_order(command.order, command.position_id)

        if command.position_id.not_null() and not self.cache.position_exists(command.position_id):
            self._invalidate_order(
                command.order.cl_ord_id,
                f"{repr(command.position_id)} does not exist",
            )
            return  # Invalid command

        # Submit order
        if self._risk_engine is not None:
            self._risk_engine.execute(command)
        else:
            client.submit_order(command)

    cdef inline void _handle_submit_bracket_order(self, ExecutionClient client, SubmitBracketOrder command) except *:
        # Validate command
        if self.cache.order_exists(command.bracket_order.entry.cl_ord_id):
            self._invalidate_bracket_order(command.bracket_order)
            return  # Invalid command
        if self.cache.order_exists(command.bracket_order.stop_loss.cl_ord_id):
            self._invalidate_bracket_order(command.bracket_order)
            return  # Invalid command
        if command.bracket_order.take_profit is not None \
                and self.cache.order_exists(command.bracket_order.take_profit.cl_ord_id):
            self._invalidate_bracket_order(command.bracket_order)
            return  # Invalid command

        # Cache all orders
        self.cache.add_order(command.bracket_order.entry, PositionId.null_c())
        self.cache.add_order(command.bracket_order.stop_loss, PositionId.null_c())
        if command.bracket_order.take_profit is not None:
            self.cache.add_order(command.bracket_order.take_profit, PositionId.null_c())

        # Submit order
        if self._risk_engine is not None:
            self._risk_engine.execute(command)
        else:
            client.submit_bracket_order(command)

    cdef inline void _handle_update_order(self, ExecutionClient client, UpdateOrder command) except *:
        # Validate command
        if not self.cache.is_order_working(command.cl_ord_id):
            self._log.warning(f"Cannot update order: "
                              f"{repr(command.cl_ord_id)} already completed.")
            return  # Invalid command

        # Amend order
        if self._risk_engine is not None:
            self._risk_engine.execute(command)
        else:
            client.update_order(command)

    cdef inline void _handle_cancel_order(self, ExecutionClient client, CancelOrder command) except *:
        # Validate command
        if self.cache.is_order_completed(command.cl_ord_id):
            self._log.warning(f"Cannot cancel order: "
                              f"{repr(command.cl_ord_id)} already completed.")
            return  # Invalid command

        # Cancel order
        if self._risk_engine is not None:
            self._risk_engine.execute(command)
        else:
            client.cancel_order(command)

    cdef inline void _invalidate_order(self, ClientOrderId cl_ord_id, str reason) except *:
        # Generate event
        cdef OrderInvalid invalid = OrderInvalid(
            cl_ord_id,
            reason,
            self._uuid_factory.generate(),
            self._clock.timestamp_ns(),
        )

        self._handle_event(invalid)

    cdef inline void _invalidate_bracket_order(self, BracketOrder bracket_order) except *:
        cdef ClientOrderId entry_id = bracket_order.entry.cl_ord_id
        cdef ClientOrderId stop_loss_id = bracket_order.stop_loss.cl_ord_id
        cdef ClientOrderId take_profit_id
        if bracket_order.take_profit:
            take_profit_id = bracket_order.take_profit.cl_ord_id

        cdef list error_msgs = []

        # Check entry ----------------------------------------------------------
        if self.cache.order_exists(entry_id):
            error_msgs.append(f"Duplicate {repr(entry_id)}")
        else:
            # Add to cache to be able to invalidate
            self.cache.add_order(bracket_order.entry, PositionId.null_c())
            self._invalidate_order(
                bracket_order.entry.cl_ord_id,
                "Duplicate ClientOrderId in bracket.",
            )
        # Check stop-loss ------------------------------------------------------
        if self.cache.order_exists(stop_loss_id):
            error_msgs.append(f"Duplicate {repr(stop_loss_id)}")
        else:
            # Add to cache to be able to invalidate
            self.cache.add_order(bracket_order.stop_loss, PositionId.null_c())
            self._invalidate_order(
                bracket_order.stop_loss.cl_ord_id,
                "Duplicate ClientOrderId in bracket.",
            )
        # Check take-profit ----------------------------------------------------
        if take_profit_id is not None and self.cache.order_exists(take_profit_id):
            error_msgs.append(f"Duplicate {repr(take_profit_id)}")
        else:
            # Add to cache to be able to invalidate
            self.cache.add_order(bracket_order.take_profit, PositionId.null_c())
            self._invalidate_order(
                bracket_order.take_profit.cl_ord_id,
                "Duplicate ClientOrderId in bracket.",
            )

        # Finally log error
        self._log.error(f"Cannot submit BracketOrder: {', '.join(error_msgs)}")

# -- EVENT HANDLERS --------------------------------------------------------------------------------

    cdef inline void _handle_event(self, Event event) except *:
        self._log.debug(f"{RECV}{EVT} {event}.")
        self.event_count += 1

        if isinstance(event, OrderEvent):
            self._handle_order_event(event)
            self._send_to_risk_engine(event)
        elif isinstance(event, PositionEvent):
            self._handle_position_event(event)
            self._send_to_risk_engine(event)
        elif isinstance(event, AccountState):
            self._handle_account_event(event)
        else:
            self._log.error(f"Cannot handle event: unrecognized {event}.")

    cdef inline void _handle_account_event(self, AccountState event) except *:
        cdef Account account = self.cache.account(event.account_id)
        if account is None:
            # Generate account
            account = Account(event)
            self.cache.add_account(account)
            self._portfolio.register_account(account)
        else:
            account.apply(event=event)
            self.cache.update_account(account)

    cdef inline void _handle_position_event(self, PositionEvent event) except *:
        self._portfolio.update_position(event)
        self._send_to_strategy(event, event.position.strategy_id)

    cdef inline void _handle_order_event(self, OrderEvent event) except *:
        if isinstance(event, (OrderCancelRejected, OrderUpdateRejected)):
            self._handle_order_command_rejected(event)
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
                self._log.error(f"Cannot apply event to any order: "
                                f"{repr(event.cl_ord_id)} and {repr(event.order_id)} "
                                f"not found in cache.")
                return  # Cannot process event further

            # Search cache for Order matching the found ClientOrderId
            order = self.cache.order(cl_ord_id)
            if order is None:
                self._log.error(f"Cannot apply event to any order: "
                                f"order for {repr(cl_ord_id)} not found in cache.")
                return  # Cannot process event further

            # Set the correct ClientOrderId for the event
            event.cl_ord_id = cl_ord_id
            self._log.info(
                f"{repr(cl_ord_id)} was found in cache and "
                f"applying event to order with {repr(order.id)}.",
                color=LogColor.GREEN,
            )

        if isinstance(event, OrderFilled):
            # The StrategyId needs to be confirmed prior to the PositionId.
            # This is in case there is no PositionId currently assigned and one
            # must be generated.
            self._confirm_strategy_id(event)
            self._confirm_position_id(event)

        try:
            # Protected against duplicate OrderFilled
            order.apply(event)
        except (KeyError, InvalidStateTrigger)  as ex:
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

    cdef inline void _confirm_strategy_id(self, OrderFilled fill) except *:
        if fill.strategy_id.not_null():
            return  # Already assigned to fill

        # Fetch identifier from cache
        cdef StrategyId strategy_id = self.cache.strategy_id_for_order(fill.cl_ord_id)
        if strategy_id is not None:
            # Assign identifier to fill
            fill.strategy_id = strategy_id
            return

        if fill.position_id.not_null():
            # Check if strategy identifier assigned for position
            strategy_id = self.cache.strategy_id_for_position(fill.position_id)
        if strategy_id is None:
            self._log.error(f"Cannot find StrategyId for "
                            f"{repr(fill.cl_ord_id)} and "
                            f"{repr(fill.position_id)} not found for {fill}.")

    cdef inline void _confirm_position_id(self, OrderFilled fill) except *:
        if fill.position_id.not_null():
            return  # Already assigned to fill

        # Fetch identifier from cache
        cdef PositionId position_id = self.cache.position_id(fill.cl_ord_id)
        if position_id is not None:
            # Assign identifier to fill
            fill.position_id = position_id
            return

        # Check for open positions
        positions_open = self.cache.positions_open(instrument_id=fill.instrument_id)
        if not positions_open:
            # Assign new identifier to fill
            fill.position_id = self._pos_id_generator.generate(fill.strategy_id)
        elif len(positions_open) == 1:
            # Assign existing identifier to fill
            fill.position_id = positions_open[0].id
        else:
            self._log.error(f"Cannot assign PositionId: "
                            f"{len(positions_open)} open positions")

    cdef inline void _handle_order_command_rejected(self, OrderEvent event) except *:
        self._send_to_strategy(event, self.cache.strategy_id_for_order(event.cl_ord_id))

    cdef inline void _handle_order_fill(self, OrderFilled fill) except *:
        cdef Position position = self.cache.position(fill.position_id)
        if position is None:  # No position open
            self._open_position(fill)
        else:
            self._update_position(position, fill)

    cdef inline void _open_position(self, OrderFilled fill) except *:
        cdef Position position = Position(fill=fill)
        self.cache.add_position(position)

        self._send_to_strategy(fill, fill.strategy_id)
        self.process(self._pos_opened_event(position, fill))

    cdef inline void _update_position(self, Position position, OrderFilled fill) except *:
        # Check for flip
        if fill.order_side != position.entry and fill.last_qty > position.quantity:
            self._flip_position(position, fill)
            return  # Handled in flip

        try:
            # Protected against duplicate OrderFilled
            position.apply(fill)
        except KeyError as ex:
            self._log.exception(ex)
            return  # Not re-raising to avoid crashing engine

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
            difference = Quantity(fill.last_qty - position.quantity)
        else:  # position.side == PositionSide.SHORT:
            difference = Quantity(abs(position.quantity - fill.last_qty))

        # Split commission between two positions
        fill_percent1 = position.quantity / fill.last_qty
        fill_percent2 = 1 - fill_percent1  # Subtract from an integer to return a Decimal

        # Split fill to close original position
        cdef OrderFilled fill_split1 = OrderFilled(
            fill.account_id,
            fill.cl_ord_id,
            fill.order_id,
            fill.execution_id,
            fill.position_id,
            fill.strategy_id,
            fill.instrument_id,
            fill.order_side,
            position.quantity,                       # Fill original position quantity remaining
            fill.last_px,
            Quantity(fill.cum_qty - difference),     # Adjust cumulative qty by difference
            Quantity(fill.leaves_qty + difference),  # Adjust leaves qty by difference
            fill.currency,
            fill.is_inverse,
            Money(fill.commission * fill_percent1, fill.commission.currency),
            fill.liquidity_side,
            fill.execution_ns,
            fill.id,
            fill.timestamp_ns,
        )

        # Close original position
        position.apply(fill_split1)
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
            fill.instrument_id,
            fill.order_side,
            difference,  # Fill difference from original as above
            fill.last_px,
            fill.cum_qty,
            fill.leaves_qty,
            fill.currency,
            fill.is_inverse,
            Money(fill.commission * fill_percent2, fill.commission.currency),
            fill.liquidity_side,
            fill.execution_ns,
            self._uuid_factory.generate(),  # New event identifier
            fill.timestamp_ns,
        )

        cdef Position position_flip = Position(fill=fill_split2)
        self.cache.add_position(position_flip)
        self.process(self._pos_opened_event(position_flip, fill_split2))

    cdef inline PositionOpened _pos_opened_event(self, Position position, OrderFilled fill):
        return PositionOpened(
            position,
            fill,
            self._uuid_factory.generate(),
            fill.timestamp_ns,
        )

    cdef inline PositionChanged _pos_changed_event(self, Position position, OrderFilled fill):
        return PositionChanged(
            position,
            fill,
            self._uuid_factory.generate(),
            fill.timestamp_ns,
        )

    cdef inline PositionClosed _pos_closed_event(self, Position position, OrderFilled fill):
        return PositionClosed(
            position,
            fill,
            self._uuid_factory.generate(),
            fill.timestamp_ns,
        )

    cdef inline void _send_to_strategy(self, Event event, StrategyId strategy_id) except *:
        if strategy_id is None:
            self._log.error(f"Cannot send event to strategy: "
                            f"{repr(strategy_id)} not found for {event}.")
            return  # Cannot send to strategy

        cdef TradingStrategy strategy = self._strategies.get(strategy_id)
        if strategy is None:
            self._log.error(f"Cannot send event to strategy: "
                            f"{repr(strategy_id)} not registered for {event}.")
            return  # Cannot send to strategy

        strategy.handle_event(event)

    cdef inline void _send_to_risk_engine(self, Event event) except *:
        # If a `RiskEngine` is registered then send the event there
        if self._risk_engine is not None:
            self._risk_engine.process(event)
