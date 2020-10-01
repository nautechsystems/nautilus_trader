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

from nautilus_trader.common.account cimport Account
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.factories cimport OrderFactory
from nautilus_trader.common.generators cimport PositionIdGenerator
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport EVT
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.portfolio cimport Portfolio
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.fsm cimport InvalidStateTrigger
from nautilus_trader.execution.cache cimport ExecutionCache
from nautilus_trader.model.commands cimport AccountInquiry
from nautilus_trader.model.commands cimport CancelAllOrders
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport Command
from nautilus_trader.model.commands cimport FlattenAllPositions
from nautilus_trader.model.commands cimport FlattenPosition
from nautilus_trader.model.commands cimport KillSwitch
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
from nautilus_trader.model.events cimport OrderRejected
from nautilus_trader.model.events cimport PositionClosed
from nautilus_trader.model.events cimport PositionEvent
from nautilus_trader.model.events cimport PositionModified
from nautilus_trader.model.events cimport PositionOpened
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport IdTag
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.order cimport MarketOrder
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.order cimport flatten_side
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class ExecutionEngine:
    """
    Provides a high performance execution engine.
    """

    def __init__(
            self,
            TraderId trader_id not None,
            AccountId account_id not None,
            ExecutionCache database not None,
            Portfolio portfolio not None,
            Clock clock not None,
            UUIDFactory uuid_factory not None,
            Logger logger not None
    ):
        """
        Initialize a new instance of the ExecutionEngine class.

        Parameters
        ----------
        trader_id : TraderId
            The trader identifier for the engine.
        account_id : AccountId
            The account identifier for the engine.
        database : ExecutionCache
            The execution cache for the engine.
        portfolio : Portfolio
            The portfolio for the engine.
        clock : Clock
            The clock for the engine.
        uuid_factory : UUIDFactory
            The uuid_factory for the engine.
        logger : Logger
            The logger for the engine.

        Raises
        ------
        ValueError
            If trader_id is not equal to the cache.trader_id.
        ValueError
            If oms_type is UNDEFINED.

        """
        Condition.equal(trader_id, database.trader_id, "trader_id", "cache.trader_id")

        self._clock = clock
        self._uuid_factory = uuid_factory
        self._log = LoggerAdapter("ExecEngine", logger)
        self._order_factory = OrderFactory(
            id_tag_trader=trader_id.tag,
            id_tag_strategy=IdTag('X'),  # Placeholder to identify engine generated orders
            clock=clock,
            initial_count=0,
        )
        self._pos_id_generator = PositionIdGenerator(trader_id.tag)
        self._exec_client = None
        self._registered_strategies = {}    # type: {StrategyId, TradingStrategy}
        self._is_kill_switch_active = False

        self.trader_id = trader_id
        self.account_id = account_id
        self.cache = database
        self.account = self.cache.get_account(account_id)
        self.portfolio = portfolio

        # Set symbol position counts
        symbol_counts = self.cache.get_symbol_position_counts()
        for symbol, count in symbol_counts.items():
            self._pos_id_generator.set_count(symbol, count)

        self.command_count = 0
        self.event_count = 0

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

        self._exec_client = exec_client
        self._log.info("Registered execution client.")

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
        Condition.not_in(strategy.id, self._registered_strategies, "strategy.id", "registered_strategies")

        strategy.register_execution_engine(self)
        self._registered_strategies[strategy.id] = strategy
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
        Condition.is_in(strategy.id, self._registered_strategies, "strategy.id", "registered_strategies")

        del self._registered_strategies[strategy.id]
        self._log.info(f"De-registered strategy {strategy}.")

    cpdef list registered_strategies(self):
        """
        Return a list of strategy_ids registered with the execution engine.

        Returns
        -------
        List[StrategyId]

        """
        return list(self._registered_strategies.keys())

# -- COMMANDS --------------------------------------------------------------------------------------

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
        self.cache.check_residuals()

    cpdef void reset(self) except *:
        """
        Reset the execution engine by clearing all stateful values.
        """
        self.cache.reset()
        self._pos_id_generator.reset()

        self.command_count = 0
        self.event_count = 0

# -- COMMAND-HANDLERS ------------------------------------------------------------------------------

    cdef void _execute_command(self, Command command) except *:
        self._log.debug(f"{RECV}{CMD} {command}.")
        self.command_count += 1

        if isinstance(command, KillSwitch):
            self._handle_kill_switch(command)
        elif isinstance(command, FlattenAllPositions):
            self._handle_flatten_all_positions(command)
        elif isinstance(command, FlattenPosition):
            self._handle_flatten_position(command)
        elif isinstance(command, CancelAllOrders):
            self._handle_cancel_all_orders(command)
        elif isinstance(command, CancelOrder):
            self._handle_cancel_order(command)
        elif isinstance(command, SubmitOrder):
            self._handle_submit_order(command)
        elif isinstance(command, SubmitBracketOrder):
            self._handle_submit_bracket_order(command)
        elif isinstance(command, ModifyOrder):
            self._handle_modify_order(command)
        elif isinstance(command, AccountInquiry):
            self._handle_account_inquiry(command)
        else:
            self._log.error(f"Cannot handle command ({command} is unrecognized).")

    cdef void _handle_kill_switch(self, KillSwitch command) except *:
        if self._is_kill_switch_active:
            self._log.error("Received KillSwitch command when kill-switch already active.")
            return

        self._is_kill_switch_active = True
        # TODO: Implement

    cdef void _handle_flatten_position(self, FlattenPosition command) except *:
        # Validate command
        self.cache.register_flattening_id(command.position_id)

        cdef Position position = self.cache.position(command.position_id)

        if position is None:
            self._log.warning(f"Cannot flatten {position.id.to_string(with_class=True)} "
                              f"(the position was not found in the cache).")
            return  # Invalid command

        if position.is_closed():
            self._log.warning(f"Cannot flatten {position.id.to_string(with_class=True)} "
                              f"(the position is already closed).")
            return  # Invalid command

        if position.id in self.cache.flattening_ids():
            self._log.warning(f"Cannot flatten {position.id.to_string(with_class=True)} "
                              f"(already flattening).")
            return  # Invalid command

        # Create flattening order
        cdef MarketOrder order = self._order_factory.market(
            position.symbol,
            flatten_side(position.side),
            position.quantity,
        )

        # Create command
        cdef SubmitOrder submit_order = SubmitOrder(
            command.trader_id,
            command.account_id,
            command.strategy_id,
            command.position_id,
            order,
            self._uuid_factory.generate(),
            self._clock.utc_now())

        self._handle_submit_order(submit_order)

    cdef void _handle_flatten_all_positions(self, FlattenAllPositions command) except *:
        # Get all open positions for the command symbol and strategy from the cache,
        # the symbol may be None in which case the query is not filtered on symbol.
        cdef set position_open_ids = self.cache.position_open_ids(
            symbol=command.symbol,
            strategy_id=command.strategy_id,
        )

        # Generate commands for all open positions
        cdef FlattenPosition flatten_cmd
        cdef PositionId position_id
        for position_id in position_open_ids:
            flatten_cmd = FlattenPosition(
                command.trader_id,
                command.account_id,
                position_id,
                command.strategy_id,
                self._uuid_factory.generate(),
                self._clock.utc_now(),
            )

            self._handle_flatten_position(flatten_cmd)

    cdef void _handle_cancel_order(self, CancelOrder command) except *:
        # Validate command
        if not self.cache.is_order_working(command.cl_ord_id):
            self._log.warning(f"Cannot cancel {command.cl_ord_id.to_string(with_class=True)} "
                              f"(already completed).")
            return  # Invalid command

        self._exec_client.cancel_order(command)

    cdef void _handle_cancel_all_orders(self, CancelAllOrders command) except *:
        # Get all working orders for the command strategy from the cache,
        # the symbol may be None in which case the query is not filtered on symbol.
        cdef set order_working_ids = self.cache.order_working_ids(
            symbol=None,
            strategy_id=command.strategy_id,
        )

        # Generate commands for all working orders
        cdef CancelOrder cancel_cmd
        cdef ClientOrderId order_id
        for order_id in order_working_ids:
            cancel_cmd = CancelOrder(
                command.trader_id,
                command.account_id,
                order_id,
                self._uuid_factory.generate(),
                self._clock.utc_now(),
            )

            self._exec_client.cancel_order(cancel_cmd)

    cdef void _handle_submit_order(self, SubmitOrder command) except *:
        # Validate command
        if self.cache.order_exists(command.order.cl_ord_id):
            self._invalidate_order(command.order, f"cl_ord_id already exists")
            return  # Invalid command

        # TODO
        # if self._oms_type == OMSType.NETTING:

        if command.position_id.not_null() and not self.cache.position_exists(command.position_id):
            self._invalidate_order(command.order, f"position_id does not exist")
            return  # Invalid command

        # Cache order
        self.cache.add_order(command.order, command.position_id, command.strategy_id)

        # Submit order
        self._exec_client.submit_order(command)

    cdef void _handle_submit_bracket_order(self, SubmitBracketOrder command) except *:
        # Validate command -------------------------------------------------------------------------
        if self.cache.order_exists(command.bracket_order.entry.cl_ord_id):
            self._invalidate_order(command.bracket_order.entry, f"cl_ord_id already exists")
            self._invalidate_order(command.bracket_order.stop_loss, "parent cl_ord_id already exists")
            if command.bracket_order.has_take_profit:
                self._invalidate_order(command.bracket_order.take_profit, "parent cl_ord_id already exists")
            return  # Invalid command
        if self.cache.order_exists(command.bracket_order.stop_loss.cl_ord_id):
            self._invalidate_order(command.bracket_order.entry, "OCO cl_ord_id already exists")
            self._invalidate_order(command.bracket_order.stop_loss, "cl_ord_id already exists")
            if command.bracket_order.has_take_profit:
                self._invalidate_order(command.bracket_order.take_profit, "OCO cl_ord_id already exists")
            return  # Invalid command
        if command.bracket_order.has_take_profit and self.cache.order_exists(command.bracket_order.take_profit.cl_ord_id):
            self._invalidate_order(command.bracket_order.entry, "OCO cl_ord_id already exists")
            self._invalidate_order(command.bracket_order.stop_loss, "OCO cl_ord_id already exists")
            self._invalidate_order(command.bracket_order.take_profit, "cl_ord_id already exists")
            return  # Invalid command
        # ------------------------------------------------------------------------------------------

        # Cache all orders
        self.cache.add_order(command.bracket_order.entry, PositionId.null(), command.strategy_id)
        self.cache.add_order(command.bracket_order.stop_loss, PositionId.null(), command.strategy_id)
        if command.bracket_order.has_take_profit:
            self.cache.add_order(command.bracket_order.take_profit, PositionId.null(), command.strategy_id)

        # Register stop-loss and take-profits
        self.cache.register_stop_loss(command.bracket_order.stop_loss)
        if command.bracket_order.has_take_profit:
            self.cache.register_take_profit(command.bracket_order.take_profit)

        # Submit bracket order
        self._exec_client.submit_bracket_order(command)

    cdef void _handle_modify_order(self, ModifyOrder command) except *:
        # Validate command
        if not self.cache.is_order_working(command.cl_ord_id):
            self._log.warning(f"Cannot modify {command.cl_ord_id.to_string(with_class=True)} "
                              f"(already completed).")
            return  # Invalid command

        self._exec_client.modify_order(command)

    cdef void _handle_account_inquiry(self, AccountInquiry command) except *:
        self._exec_client.account_inquiry(command)

    cdef void _invalidate_order(self, Order order, str reason) except *:
        # Generate event
        cdef OrderInvalid invalid = OrderInvalid(
            order.cl_ord_id,
            reason,
            self._uuid_factory.generate(),
            self._clock.utc_now())

        self._handle_event(invalid)

    cdef void _deny_order(self, Order order, str reason) except *:
        # Generate event
        cdef OrderDenied denied = OrderDenied(
            order.cl_ord_id,
            reason,
            self._uuid_factory.generate(),
            self._clock.utc_now())

        self._handle_event(denied)

# -- EVENT-HANDLERS --------------------------------------------------------------------------------

    cdef void _handle_event(self, Event event) except *:
        self._log.debug(f"{RECV}{EVT} {event}.")
        self.event_count += 1

        if isinstance(event, OrderEvent):
            if isinstance(event, OrderCancelReject):
                self._handle_order_cancel_reject(event)
            else:
                self._handle_order_event(event)
        elif isinstance(event, PositionEvent):
            self._handle_position_event(event)
        elif isinstance(event, AccountState):
            self._handle_account_event(event)
        else:
            self._log.error(f"Cannot handle event ({event} is unrecognized).")

    cdef void _handle_order_cancel_reject(self, OrderCancelReject event) except *:
        cdef StrategyId strategy_id = self.cache.strategy_id_for_order(event.cl_ord_id)
        if not strategy_id:
            self._log.error(f"Cannot process event {event}, "
                            f"{strategy_id.to_string(with_class=True)} "
                            f"not found.")
            return  # Cannot process event further

        self._send_to_strategy(event, strategy_id)

    cdef void _handle_order_event(self, OrderEvent event) except *:
        if isinstance(event, OrderRejected):
            self._log.warning(f"{RECV}{EVT} {event}.")
        elif isinstance(event, OrderCancelReject):
            self._log.warning(f"{RECV}{EVT} {event}.")
        else:
            self._log.info(f"{RECV}{EVT} {event}.")

        cdef Order order = self.cache.order(event.cl_ord_id)
        if not order:
            self._log.warning(f"Cannot apply event {event} to any order, "
                              f"{event.cl_ord_id.to_string(with_class=True)} "
                              f"not found in cache.")
            return  # Cannot process event further

        try:
            order.apply(event)
        except InvalidStateTrigger as ex:
            self._log.exception(ex)

        self.cache.update_order(order)

        # Remove order from registered orders
        if isinstance(event, OrderEvent) and event.is_completion_trigger:
            self.cache.discard_stop_loss_id(event.cl_ord_id)
            self.cache.discard_take_profit_id(event.cl_ord_id)

        if isinstance(event, OrderRejected):
            self._handle_order_reject(event)
        elif isinstance(event, OrderFilled):
            self._handle_order_fill(event)
            return  # _handle_order_fill(event) will send to strategy (refactor)

        self._send_to_strategy(event, self.cache.strategy_id_for_order(event.cl_ord_id))

    cdef void _handle_order_reject(self, OrderRejected event) except *:
        if event.cl_ord_id not in self.cache.stop_loss_ids() and event.cl_ord_id not in self.cache.take_profit_ids():
            return  # Not a registered order

        # Find position_id for order
        cdef PositionId position_id = self.cache.position_id(event.cl_ord_id)
        if position_id is None:
            self._log.error(f"Cannot find PositionId for {event.cl_ord_id}.")  # Cannot flatten
            return

        # Flatten if open position
        if self.cache.is_position_open(position_id):
            self._log.warning(f"Rejected {event.cl_ord_id} was a registered child order, now flattening {position_id}.")
            self._handle_flatten_position(position_id)

    cdef void _handle_order_fill(self, OrderFilled fill) except *:
        # Get PositionId corresponding to fill
        cdef PositionId position_id = self.cache.position_id(fill.cl_ord_id)
        # --- position_id could be None here (position not opened yet) ---

        # Get StrategyId corresponding to fill
        cdef StrategyId strategy_id = self.cache.strategy_id_for_order(fill.cl_ord_id)
        if strategy_id is None and fill.position_id.not_null():
            strategy_id = self.cache.strategy_id_for_position(fill.position_id)
        if strategy_id is None:
            self._log.error(f"Cannot process event {fill}, StrategyId for "
                            f"{fill.cl_ord_id.to_string(with_class=True)} or"
                            f"{fill.position_id.to_string(with_class=True)} not found.")
            return  # Cannot process event further

        if fill.position_id is None:  # Exchange not assigning position_ids
            self._fill_pos_id_none(position_id, fill, strategy_id)
        else:
            self._fill_pos_id(position_id, fill, strategy_id)

    cdef void _fill_pos_id_none(self, PositionId position_id, OrderFilled fill, StrategyId strategy_id) except *:
        if position_id.is_null():  # No position yet
            # Generate identifier
            position_id = self._pos_id_generator.generate(fill.symbol)
            fill = fill.clone(new_position_id=position_id)

            # Create new position
            self._open_position(fill, strategy_id)
        else:  # Position exists
            fill = fill.clone(new_position_id=position_id)
            self._update_position(fill, strategy_id)

    cdef void _fill_pos_id(self, PositionId position_id, OrderFilled fill, StrategyId strategy_id) except *:
        if position_id is None:  # No position
            self._open_position(fill, strategy_id)
        else:
            self._update_position(fill, strategy_id)

    cdef void _open_position(self, OrderFilled fill, StrategyId strategy_id) except *:
        cdef Position position = Position(fill)
        self.cache.add_position(position, strategy_id)
        # self.cache.index_position_id(position_id, fill.cl_ord_id, strategy_id)

        self._send_to_strategy(fill, strategy_id)
        self.process(self._pos_opened_event(position, fill, strategy_id))

    cdef void _update_position(self, OrderFilled fill, StrategyId strategy_id) except *:
        cdef Position position = self.cache.position(fill.position_id)

        if position is None:
            self._log.error(f"Cannot update position for "
                            f"{fill.position_id.to_string(with_class=True)} "
                            f"(no position found in cache).")
            return

        position.apply(fill)
        self.cache.update_position(position)

        cdef PositionEvent position_event
        if position.is_closed():
            position_event = self._pos_closed_event(position, fill, strategy_id)
        else:
            position_event = self._pos_modified_event(position, fill, strategy_id)

        self._send_to_strategy(fill, strategy_id)
        self.process(position_event)

    cdef void _handle_position_event(self, PositionEvent event) except *:
        self.portfolio.update(event)
        self._send_to_strategy(event, event.strategy_id)

    cdef void _handle_account_event(self, AccountState event) except *:
        cdef Account account = self.cache.get_account(event.account_id)
        if account is None:
            account = Account(event)
            if self.account_id.equals(account.id):
                self.account = account
                self.cache.add_account(self.account)
                self.portfolio.set_base_currency(event.currency)
                return
        elif account.id == event.account_id:
            account.apply(event)
            self.cache.update_account(account)
            return

        self._log.warning(f"Cannot process event {event}, "
                          f"event {event.account_id.to_string(with_class=True)} "
                          f"does not match traders {self.account_id.to_string(with_class=True)}.")

    cdef PositionOpened _pos_opened_event(
            self,
            Position position,
            OrderFilled event,
            StrategyId strategy_id,
    ):
        return PositionOpened(
            position,
            event,
            strategy_id,
            self._uuid_factory.generate(),
            event.timestamp,
        )

    cdef PositionModified _pos_modified_event(
            self,
            Position position,
            OrderFilled event,
            StrategyId strategy_id,
    ):
        return PositionModified(
            position,
            event,
            strategy_id,
            self._uuid_factory.generate(),
            event.timestamp,
        )

    cdef PositionClosed _pos_closed_event(
            self,
            Position position,
            OrderFilled event,
            StrategyId strategy_id,
    ):
        return PositionClosed(
            position,
            event,
            strategy_id,
            self._uuid_factory.generate(),
            event.timestamp,
        )

    cdef void _send_to_strategy(self, Event event, StrategyId strategy_id) except *:
        if strategy_id is None:
            self._log.error(f"Cannot send event {event} to strategy, "
                            f"{strategy_id.to_string(with_class=True)} not found.")
            return  # Cannot send to strategy

        cdef TradingStrategy strategy = self._registered_strategies.get(strategy_id)
        if strategy_id is None:
            self._log.error(f"Cannot send event {event} to strategy, "
                            f"{strategy_id.to_string(with_class=True)} not registered.")
            return

        strategy.handle_event(event)

    cdef void _reset(self) except *:
        """
        Reset the execution engine to its initial state.
        """
        self._registered_strategies.clear()
        self._pos_id_generator.reset()
        self.command_count = 0
        self.event_count = 0
