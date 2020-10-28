# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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
The `ExecutionEngine` is the central component of the entire execution stack for the platform.

Its primary responsibility is to orchestrate interactions between the individual
`DataClient` instances, and the rest of the platform. This is could include
ongoing subscriptions to specific data types, for particular endpoints.

Beneath it sits the `DataCache` layer which presents a read-only facade
to its clients to consume cached data through.
"""

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.generators cimport PositionIdGenerator
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport EVT
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.decimal cimport Decimal
from nautilus_trader.core.fsm cimport InvalidStateTrigger
from nautilus_trader.execution.cache cimport ExecutionCache
from nautilus_trader.execution.database cimport ExecutionDatabase
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport Command
from nautilus_trader.model.commands cimport ModifyOrder
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
from nautilus_trader.model.events cimport PositionModified
from nautilus_trader.model.events cimport PositionOpened
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order cimport Order
from nautilus_trader.trading.account cimport Account
from nautilus_trader.trading.portfolio cimport Portfolio
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class ExecutionEngine:
    """
    Provides a high-performance execution engine for the management of many
    `ExecutionClient` instances, and the asynchronous ingest of trading related
    events.
    """

    def __init__(
            self,
            ExecutionDatabase database not None,
            Portfolio portfolio not None,
            Clock clock not None,
            UUIDFactory uuid_factory not None,
            Logger logger not None,
            dict config=None,
    ):
        """
        Initialize a new instance of the ExecutionEngine class.

        Parameters
        ----------
        database : ExecutionDatabase
            The execution database for the engine.
        portfolio : Portfolio
            The portfolio for the engine.
        clock : Clock
            The clock for the engine.
        uuid_factory : UUIDFactory
            The uuid_factory for the engine.
        logger : Logger
            The logger for the engine.
        config : dict, option
            The configuration options.

        """
        if config is None:
            config = {}

        self._clock = clock
        self._uuid_factory = uuid_factory
        self._log = LoggerAdapter("ExecEngine", logger)
        self._trader_id = database.trader_id
        self._cache = ExecutionCache(database, logger)
        self._portfolio = portfolio
        self._pos_id_generator = PositionIdGenerator(database.trader_id.tag)
        self._clients = {}     # type: {Venue, ExecutionClient}

        # Handlers
        self._strategies = {}  # type: {StrategyId, TradingStrategy}

        # Counters
        self._command_count = 0
        self._event_count = 0

    @property
    def trader_id(self):
        """
        The trader identifier associated with the engine.

        Returns
        -------
        TraderId

        """
        return self._trader_id

    @property
    def cache(self):
        """
        The engines execution cache.

        Returns
        -------
        ExecutionCache

        """
        return self._cache

    @property
    def portfolio(self):
        """
        The

        Returns
        -------
        Portfolio

        """
        return self._portfolio

    @property
    def command_count(self):
        """
        The total count of commands received by the engine.

        Returns
        -------
        int

        """
        return self._command_count

    @property
    def event_count(self):
        """
        The total count of events received by the engine.

        Returns
        -------
        int

        """
        return self._event_count

# -- REGISTRATIONS ---------------------------------------------------------------------------------

    cpdef void register_client(self, ExecutionClient exec_client) except *:
        """
        Register the given execution client with the execution engine.

        Parameters
        ----------
        exec_client : ExecutionClient
            The execution client to register.

        """
        Condition.not_none(exec_client, "exec_client")
        Condition.not_in(exec_client.venue, self._clients, "exec_client.venue", "_clients")

        self._clients[exec_client.venue] = exec_client
        self._log.info(f"Registered execution client for the {exec_client.venue} venue.")

    cpdef void deregister_client(self, ExecutionClient exec_client) except *:
        """
        Deregister the given execution client from the execution engine.

        Parameters
        ----------
        exec_client : ExecutionClient
            The execution client to deregister.

        """
        Condition.not_none(exec_client, "exec_client")
        Condition.is_in(exec_client.venue, self._clients, "exec_client.venue", "_clients")

        del self._clients[exec_client.venue]
        self._log.info(f"De-registered execution client for the {exec_client.venue} venue.")

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
        self._strategies[strategy.id] = strategy
        self._log.info(f"Registered strategy {strategy}.")

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
        self._log.info(f"De-registered strategy {strategy}.")

    cpdef set registered_venues(self):
        """
        Return the trading venues registered with the execution engine.

        Returns
        -------
        set[StrategyId]

        """
        return set(self._clients.keys())

    cpdef set registered_strategies(self):
        """
        Return the strategy_ids registered with the execution engine.

        Returns
        -------
        set[StrategyId]

        """
        return set(self._strategies.keys())

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void load_cache(self) except *:
        """
        Load the cache up from the execution database.
        """
        self._cache.cache_accounts()
        self._cache.cache_orders()
        self._cache.cache_positions()
        self._cache.build_index()
        self._set_position_symbol_counts()

        # Update portfolio
        self._portfolio.update_orders_working(set(self._cache.orders_working()))
        self._portfolio.update_positions(set(self._cache.positions_open()))

    cpdef void integrity_check(self) except *:
        """
        Check integrity of data within the execution cache and database.
        """
        self._cache.integrity_check()

    cpdef void _set_position_symbol_counts(self) except *:
        # For the internal position identifier generator
        cdef list positions = self._cache.positions()

        # Count positions per symbol
        cdef dict counts = {}  # type: {Symbol: int}
        cdef Position position
        for position in positions:
            if position.symbol not in counts:
                counts[position.symbol] = 0
            counts[position.symbol] += 1

        # Reset position identifier generator
        self._pos_id_generator.reset()

        # Set counts
        cdef Symbol symbol
        cdef int count
        for symbol, count in counts.items():
            self._pos_id_generator.set_count(symbol, count)
            self._log.info(f"Set position count {symbol} to {count}")

    cpdef void execute(self, Command command) except *:
        """
        Execute the given command.

        Parameters
        ----------
        command : Command
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
        self._cache.check_residuals()

    cpdef void reset(self) except *:
        """
        Reset the execution engine by clearing all stateful values.
        """
        for client in self._clients.values():
            client.reset()
        self._cache.reset()
        self._pos_id_generator.reset()

        self._command_count = 0
        self._event_count = 0

    cpdef void dispose(self) except *:
        """
        Dispose all execution clients.
        """
        self._log.info("Disposing all clients...")

        cdef ExecutionClient client
        for client in self._clients:
            client.dispose()

    cpdef void flush_db(self) except *:
        """
        Flush the execution database which permanently removes all persisted data.

        WARNING: Permanent data loss.

        """
        self._cache.flush_db()

# -- COMMAND-HANDLERS ------------------------------------------------------------------------------

    cdef inline void _execute_command(self, Command command) except *:
        self._log.debug(f"{RECV}{CMD} {command}.")
        self._command_count += 1

        if isinstance(command, Connect):
            self._handle_connect(command)
        elif isinstance(command, Disconnect):
            self._handle_disconnect(command)
        if isinstance(command, SubmitOrder):
            self._handle_submit_order(command)
        elif isinstance(command, SubmitBracketOrder):
            self._handle_submit_bracket_order(command)
        elif isinstance(command, ModifyOrder):
            self._handle_modify_order(command)
        elif isinstance(command, CancelOrder):
            self._handle_cancel_order(command)
        else:
            self._log.error(f"Cannot handle command ({command} is unrecognized).")

    cdef inline void _handle_connect(self, Connect command) except *:
        self._log.info("Connecting all clients...")

        cdef ExecutionClient client
        if command.venue is not None:
            client = self._clients.get(command.venue)
            if client is None:
                self._log.error(f"Cannot execute {command} "
                                f"(venue {command.venue} not registered).")
            else:
                client.connect()
        else:
            for client in self._clients:
                client.connect()

    cdef inline void _handle_disconnect(self, Disconnect command) except *:
        self._log.info("Disconnecting all clients...")

        cdef ExecutionClient client
        if command.venue is not None:
            client = self._clients.get(command.venue)
            if client is None:
                self._log.error(f"Cannot execute {command} "
                                f"(venue {command.venue} not registered).")
            else:
                client.disconnect()
        else:
            for client in self._clients:
                client.disconnect()

    cdef inline void _handle_submit_order(self, SubmitOrder command) except *:
        cdef ExecutionClient client = self._clients.get(command.venue)
        if client is None:
            self._log.error(f"Cannot execute {command} "
                            f"(venue {command.venue} not registered).")
            return

        # Validate command
        if self._cache.order_exists(command.order.cl_ord_id):
            self._invalidate_order(command.order, f"cl_ord_id already exists")
            return  # Invalid command

        if command.position_id.not_null and not self._cache.position_exists(command.position_id):
            self._invalidate_order(command.order, f"position_id does not exist")
            return  # Invalid command

        # Cache order
        self._cache.add_order(command.order, command.position_id)

        # Submit order
        client.submit_order(command)

    cdef inline void _handle_submit_bracket_order(self, SubmitBracketOrder command) except *:
        cdef ExecutionClient client = self._clients.get(command.venue)
        if client is None:
            self._log.error(f"Cannot execute {command} "
                            f"(venue {command.venue} not registered).")
            return

        # Validate command
        if self._cache.order_exists(command.bracket_order.entry.cl_ord_id):
            self._invalidate_order(command.bracket_order.entry, f"cl_ord_id already exists")
            self._invalidate_order(command.bracket_order.stop_loss, "parent cl_ord_id already exists")
            if command.bracket_order.has_take_profit:
                self._invalidate_order(command.bracket_order.take_profit, "parent cl_ord_id already exists")
            return  # Invalid command
        if self._cache.order_exists(command.bracket_order.stop_loss.cl_ord_id):
            self._invalidate_order(command.bracket_order.entry, "OCO cl_ord_id already exists")
            self._invalidate_order(command.bracket_order.stop_loss, "cl_ord_id already exists")
            if command.bracket_order.has_take_profit:
                self._invalidate_order(command.bracket_order.take_profit, "OCO cl_ord_id already exists")
            return  # Invalid command
        if command.bracket_order.has_take_profit and self._cache.order_exists(command.bracket_order.take_profit.cl_ord_id):
            self._invalidate_order(command.bracket_order.entry, "OCO cl_ord_id already exists")
            self._invalidate_order(command.bracket_order.stop_loss, "OCO cl_ord_id already exists")
            self._invalidate_order(command.bracket_order.take_profit, "cl_ord_id already exists")
            return  # Invalid command

        # Cache all orders
        self._cache.add_order(command.bracket_order.entry, PositionId.null())
        self._cache.add_order(command.bracket_order.stop_loss, PositionId.null())
        if command.bracket_order.has_take_profit:
            self._cache.add_order(command.bracket_order.take_profit, PositionId.null())

        # Submit bracket order
        client.submit_bracket_order(command)

    cdef inline void _handle_modify_order(self, ModifyOrder command) except *:
        cdef ExecutionClient client = self._clients.get(command.venue)
        if client is None:
            self._log.error(f"Cannot execute {command} "
                            f"(venue {command.venue} not registered).")
            return

        # Validate command
        if not self._cache.is_order_working(command.cl_ord_id):
            self._log.warning(f"Cannot modify {repr(command.cl_ord_id)} "
                              f"(already completed).")
            return  # Invalid command

        client.modify_order(command)

    cdef inline void _handle_cancel_order(self, CancelOrder command) except *:
        cdef ExecutionClient client = self._clients.get(command.venue)
        if client is None:
            self._log.error(f"Cannot execute {command} "
                            f"(venue {command.venue} not registered).")
            return

        # Validate command
        if self._cache.is_order_completed(command.cl_ord_id):
            self._log.warning(f"Cannot cancel {repr(command.cl_ord_id)} "
                              f"(already completed).")
            return  # Invalid command

        client.cancel_order(command)

    cdef inline void _invalidate_order(self, Order order, str reason) except *:
        # Generate event
        cdef OrderInvalid invalid = OrderInvalid(
            order.cl_ord_id,
            reason,
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self._handle_event(invalid)

    cdef inline void _deny_order(self, Order order, str reason) except *:
        # Generate event
        cdef OrderDenied denied = OrderDenied(
            order.cl_ord_id,
            reason,
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self._handle_event(denied)

# -- EVENT-HANDLERS --------------------------------------------------------------------------------

    cdef inline void _handle_event(self, Event event) except *:
        self._log.debug(f"{RECV}{EVT} {event}.")
        self._event_count += 1

        if isinstance(event, OrderEvent):
            self._handle_order_event(event)
        elif isinstance(event, PositionEvent):
            self._handle_position_event(event)
        elif isinstance(event, AccountState):
            self._handle_account_event(event)
        else:
            self._log.error(f"Cannot handle event ({event} is unrecognized).")

    cdef inline void _handle_account_event(self, AccountState event) except *:
        cdef Account account = self._cache.account(event.account_id)
        if account is None:
            # Generate account
            account = Account(event)
            self._cache.add_account(account)
            self._portfolio.register_account(account)
        else:
            account.apply(event)
            self._cache.update_account(account)

    cdef inline void _handle_position_event(self, PositionEvent event) except *:
        self._portfolio.update_position(event)
        self._send_to_strategy(event, event.position.strategy_id)

    cdef inline void _handle_order_event(self, OrderEvent event) except *:
        if isinstance(event, OrderCancelReject):
            self._handle_order_cancel_reject(event)
            return  # Sent to strategy

        cdef Order order = self._cache.order(event.cl_ord_id)
        if order is None:
            self._log.warning(f"Cannot apply event {event} to any order, "
                              f"{repr(event.cl_ord_id)} "
                              f"not found in cache.")
            return  # Cannot process event further

        try:
            order.apply(event)
        except InvalidStateTrigger as ex:
            self._log.exception(ex)

        self._cache.update_order(order)

        # Update portfolio
        if order.is_working or order.is_completed:
            self._portfolio.update_order(order)

        if isinstance(event, OrderFilled):
            self._handle_order_fill(event)
            return  # Sent to strategy

        self._send_to_strategy(event, self._cache.strategy_id_for_order(event.cl_ord_id))

    cdef inline void _handle_order_cancel_reject(self, OrderCancelReject event) except *:
        cdef StrategyId strategy_id = self._cache.strategy_id_for_order(event.cl_ord_id)
        if strategy_id is None:
            self._log.error(f"Cannot process event {event}, "
                            f"{repr(strategy_id)} "
                            f"not found.")
            return  # Cannot process event further

        self._send_to_strategy(event, strategy_id)

    cdef inline void _handle_order_fill(self, OrderFilled fill) except *:
        # Get PositionId corresponding to fill
        cdef PositionId position_id = self._cache.position_id(fill.cl_ord_id)
        # --- position_id could be None here (position not opened yet) ---

        # Get StrategyId corresponding to fill
        cdef StrategyId strategy_id = self._cache.strategy_id_for_order(fill.cl_ord_id)
        if strategy_id is None and fill.position_id.not_null:
            strategy_id = self._cache.strategy_id_for_position(fill.position_id)
        if strategy_id is None:
            self._log.error(f"Cannot process event {fill}, StrategyId for "
                            f"{repr(fill.cl_ord_id)} or"
                            f"{repr(fill.position_id)} not found.")
            return  # Cannot process event further

        if fill.position_id.is_null:  # Exchange not assigning position_ids
            self._fill_system_assigned_ids(position_id, fill, strategy_id)
        else:
            self._fill_exchange_assigned_ids(position_id, fill, strategy_id)

    cdef inline void _fill_system_assigned_ids(
            self,
            PositionId position_id,
            OrderFilled fill,
            StrategyId strategy_id,
    ) except *:
        if position_id.is_null:  # No position yet
            # Generate identifier
            position_id = self._pos_id_generator.generate(fill.symbol)
            fill = fill.clone(position_id=position_id, strategy_id=strategy_id)

            # Create new position
            self._open_position(fill)
        else:  # Position exists
            fill = fill.clone(position_id=position_id, strategy_id=strategy_id)
            self._update_position(fill)

    cdef inline void _fill_exchange_assigned_ids(
            self,
            PositionId position_id,
            OrderFilled fill,
            StrategyId strategy_id,
    ) except *:
        fill = fill.clone(position_id=fill.position_id, strategy_id=strategy_id)
        if position_id is None:  # No position
            self._open_position(fill)
        else:
            self._update_position(fill)

    cdef inline void _open_position(self, OrderFilled fill) except *:
        cdef Position position = Position(fill)
        self._cache.add_position(position)

        self._send_to_strategy(fill, fill.strategy_id)
        self.process(self._pos_opened_event(position, fill))

    cdef inline void _update_position(self, OrderFilled fill) except *:
        cdef Position position = self._cache.position(fill.position_id)
        if position is None:
            self._log.error(f"Cannot update position for "
                            f"{repr(fill.position_id)} "
                            f"(no position found in cache).")
            return  # Cannot process event further

        # Check for flip
        if fill.order_side != position.entry and fill.filled_qty > position.quantity:
            self._flip_position(position, fill)
            return  # Handled in flip

        position.apply(fill)
        self._cache.update_position(position)

        cdef PositionEvent position_event
        if position.is_closed:
            position_event = self._pos_closed_event(position, fill)
        else:
            position_event = self._pos_modified_event(position, fill)

        self._send_to_strategy(fill, fill.strategy_id)
        self.process(position_event)

    cdef inline void _flip_position(self, Position position, OrderFilled fill) except *:
        cdef Quantity difference
        if position.side == PositionSide.LONG:
            difference = Quantity(fill.filled_qty - position.quantity)
        else:  # position.side == PositionSide.SHORT:
            difference = Quantity(position.quantity - fill.filled_qty)

        # Split commission between two positions
        cdef Decimal fill_percent1 = position.quantity / fill.filled_qty
        cdef Decimal fill_percent2 = 1 - fill_percent1

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
            position.quantity,  # Fill original position quantity remaining
            position.quantity,  # Cumulative quantity is fill quantity
            fill.leaves_qty,
            fill.avg_price,
            Money(fill.commission * fill_percent1, fill.commission.currency),
            fill.liquidity_side,
            fill.base_currency,
            fill.quote_currency,
            fill.is_inverse,
            fill.execution_time,
            fill.id,
            fill.timestamp,
        )

        # Close original position
        position.apply(fill_split1)
        self._cache.update_position(position)

        self._send_to_strategy(fill, fill.strategy_id)
        self.process(self._pos_closed_event(position, fill))

        # Generate position identifier for flipped position
        cdef PositionId position_id_flip = self._pos_id_generator.generate(
            symbol=fill.symbol,
            flipped=True,
        )

        # Split fill to open flipped position
        cdef OrderFilled fill_split2 = OrderFilled(
            fill.account_id,
            ClientOrderId(fill.cl_ord_id.value + 'F'),
            fill.order_id,
            fill.execution_id,
            position_id_flip,
            fill.strategy_id,
            fill.symbol,
            fill.order_side,
            difference,  # Fill difference from original as above
            difference,  # Cumulative quantity is fill quantity
            fill.leaves_qty,
            fill.avg_price,
            Money(fill.commission * fill_percent2, fill.commission.currency),
            fill.liquidity_side,
            fill.base_currency,
            fill.quote_currency,
            fill.is_inverse,
            fill.execution_time,
            self._uuid_factory.generate(),  # New event identifier
            fill.timestamp,
        )

        cdef Position position_flip = Position(fill_split2)
        self._cache.add_position(position_flip)
        self.process(self._pos_opened_event(position_flip, fill_split2))

    cdef inline PositionOpened _pos_opened_event(self, Position position, OrderFilled event):
        return PositionOpened(
            position,
            event,
            self._uuid_factory.generate(),
            event.timestamp,
        )

    cdef inline PositionModified _pos_modified_event(self, Position position, OrderFilled event):
        return PositionModified(
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
            self._log.error(f"Cannot send event {event} to strategy, "
                            f"{repr(strategy_id)} not found.")
            return  # Cannot send to strategy

        cdef TradingStrategy strategy = self._strategies.get(strategy_id)
        if strategy is None:
            self._log.error(f"Cannot send event {event} to strategy, "
                            f"{repr(strategy_id)} not registered.")
            return  # Cannot send to strategy

        strategy.handle_event(event)
