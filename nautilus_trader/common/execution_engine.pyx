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

from cpython.datetime cimport datetime

from nautilus_trader.common.account cimport Account
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport EVT
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.portfolio cimport Portfolio
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.fsm cimport InvalidStateTrigger
from nautilus_trader.model.commands cimport AccountInquiry
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
from nautilus_trader.model.events cimport OrderFillEvent
from nautilus_trader.model.events cimport OrderInvalid
from nautilus_trader.model.events cimport PositionClosed
from nautilus_trader.model.events cimport PositionEvent
from nautilus_trader.model.events cimport PositionModified
from nautilus_trader.model.events cimport PositionOpened
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientPositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.order cimport Order
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class ExecutionEngine:
    """
    Provides a generic execution engine.
    """

    def __init__(self,
                 TraderId trader_id not None,
                 AccountId account_id not None,
                 ExecutionDatabase database not None,
                 Portfolio portfolio not None,
                 Clock clock not None,
                 UUIDFactory uuid_factory not None,
                 Logger logger not None):
        """
        Initialize a new instance of the ExecutionEngine class.

        :param trader_id: The trader identifier for the engine.
        :param account_id: The account identifier for the engine.
        :param database: The execution database for the engine.
        :param portfolio: The portfolio for the engine.
        :param clock: The clock for the engine.
        :param uuid_factory: The uuid_factory for the engine.
        :param logger: The logger for the engine.
        :raises ValueError: If trader_id is not equal to the database.trader_id.
        """
        Condition.equal(trader_id, database.trader_id, "trader_id", "database.trader_id")

        self._clock = clock
        self._uuid_factory = uuid_factory
        self._log = LoggerAdapter("ExecEngine", logger)

        self._registered_strategies = {}  # type: {StrategyId, TradingStrategy}
        self._exec_client = None

        self.trader_id = trader_id
        self.account_id = account_id
        self.database = database
        self.account = self.database.get_account(self.account_id)
        self.portfolio = portfolio

        self.command_count = 0
        self.event_count = 0

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void register_client(self, ExecutionClient exec_client) except *:
        """
        Register the given execution client with the execution engine.

        :param exec_client: The execution client to register.
        """
        Condition.not_none(exec_client, "exec_client")

        self._exec_client = exec_client
        self._log.info("Registered execution client.")

    cpdef void register_strategy(self, TradingStrategy strategy) except *:
        """
        Register the given strategy with the execution engine.

        :param strategy: The strategy to register.
        :raises ValueError: If strategy is already registered with the execution engine.
        """
        Condition.not_none(strategy, "strategy")
        Condition.not_in(strategy.id, self._registered_strategies, "strategy.id", "registered_strategies")

        strategy.register_execution_engine(self)
        self._registered_strategies[strategy.id] = strategy
        self._log.info(f"Registered strategy {strategy}.")

    cpdef void deregister_strategy(self, TradingStrategy strategy) except *:
        """
        Deregister the given strategy with the execution engine.

        :param strategy: The strategy to deregister.
        :raises ValueError: If strategy is not registered with the execution client.
        """
        Condition.not_none(strategy, "strategy")
        Condition.is_in(strategy.id, self._registered_strategies, "strategy.id", "registered_strategies")

        del self._registered_strategies[strategy.id]
        self._log.info(f"De-registered strategy {strategy}.")

    cpdef void execute_command(self, Command command) except *:
        """
        Execute the given command.

        :param command: The command to execute.
        """
        Condition.not_none(command, "command")

        self._execute_command(command)

    cpdef void handle_event(self, Event event) except *:
        """
        Handle the given command.

        :param event: The event to handle.
        """
        Condition.not_none(event, "event")

        self._handle_event(event)

    cpdef void check_residuals(self) except *:
        """
        Check for residual working orders or open positions.
        """
        self.database.check_residuals()

    cpdef void reset(self) except *:
        """
        Reset the execution engine by clearing all stateful values.
        """
        self.database.reset()

        self.command_count = 0
        self.event_count = 0

# -- QUERIES ---------------------------------------------------------------------------------------

    cpdef list registered_strategies(self):
        """
        Return a list of strategy_ids registered with the execution engine.

        :return List[StrategyId].
        """
        return list(self._registered_strategies.keys())

    cpdef bint is_strategy_flat(self, StrategyId strategy_id):
        """
        Return a value indicating whether the strategy given identifier is flat
        (all associated positions FLAT).

        :param strategy_id: The strategy_id.
        :return bool.
        """
        Condition.not_none(strategy_id, "strategy_id")

        return self.database.count_positions_open(strategy_id) == 0

    cpdef bint is_flat(self):
        """
        Return a value indicating whether the execution engine is flat.

        :return bool.
        """
        return self.database.count_positions_open() == 0

# --------------------------------------------------------------------------------------------------

    cdef void _execute_command(self, Command command) except *:
        self._log.debug(f"{RECV}{CMD} {command}.")
        self.command_count += 1

        if isinstance(command, AccountInquiry):
            self._handle_account_inquiry(command)
        elif isinstance(command, SubmitOrder):
            self._handle_submit_order(command)
        elif isinstance(command, SubmitBracketOrder):
            self._handle_submit_bracket_order(command)
        elif isinstance(command, ModifyOrder):
            self._handle_modify_order(command)
        elif isinstance(command, CancelOrder):
            self._handle_cancel_order(command)
        else:
            self._log.error(f"Cannot handle command ({command} is unrecognized).")

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

    cdef void _handle_account_inquiry(self, AccountInquiry command) except *:
        self._exec_client.account_inquiry(command)

    cdef void _handle_submit_order(self, SubmitOrder command) except *:
        # Validate order identifier
        if self.database.order_exists(command.order.cl_ord_id):
            self._invalidate_order(command.order, f"cl_ord_id already exists")
            return  # Cannot submit order

        # Submit order
        self.database.add_order(command.order, command.strategy_id, command.cl_pos_id)
        self._exec_client.submit_order(command)

    cdef void _handle_submit_bracket_order(self, SubmitBracketOrder command) except *:
        # Validate order identifiers
        if self.database.order_exists(command.bracket_order.entry.cl_ord_id):
            self._invalidate_order(command.bracket_order.entry, f"cl_ord_id already exists")
            self._invalidate_order(command.bracket_order.stop_loss, "parent cl_ord_id already exists")
            if command.bracket_order.has_take_profit:
                self._invalidate_order(command.bracket_order.take_profit, "parent cl_ord_id already exists")
            return  # Cannot submit order
        if self.database.order_exists(command.bracket_order.stop_loss.cl_ord_id):
            self._invalidate_order(command.bracket_order.entry, "OCO cl_ord_id already exists")
            self._invalidate_order(command.bracket_order.stop_loss, "cl_ord_id already exists")
            if command.bracket_order.has_take_profit:
                self._invalidate_order(command.bracket_order.take_profit, "OCO cl_ord_id already exists")
            return  # Cannot submit order
        if command.bracket_order.has_take_profit and self.database.order_exists(command.bracket_order.take_profit.cl_ord_id):
            self._invalidate_order(command.bracket_order.entry, "OCO cl_ord_id already exists")
            self._invalidate_order(command.bracket_order.stop_loss, "OCO cl_ord_id already exists")
            self._invalidate_order(command.bracket_order.take_profit, "cl_ord_id already exists")
            return  # Cannot submit order

        # Submit order
        self.database.add_order(command.bracket_order.entry, command.strategy_id, command.cl_pos_id)
        self.database.add_order(command.bracket_order.stop_loss, command.strategy_id, command.cl_pos_id)
        if command.bracket_order.has_take_profit:
            self.database.add_order(command.bracket_order.take_profit, command.strategy_id, command.cl_pos_id)
        self._exec_client.submit_bracket_order(command)

    cdef void _handle_modify_order(self, ModifyOrder command) except *:
        self._exec_client.modify_order(command)

    cdef void _handle_cancel_order(self, CancelOrder command) except *:
        self._exec_client.cancel_order(command)

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
        cdef StrategyId strategy_id = self.database.get_strategy_for_order(event.cl_ord_id)
        if strategy_id is None:
            self._log.error(f"Cannot process event {event}, "
                            f"{strategy_id.to_string(with_class=True)} "
                            f"not found.")
            return  # Cannot process event further

        self._send_to_strategy(event, strategy_id)

    cdef void _handle_order_event(self, OrderEvent event) except *:
        cdef Order order = self.database.get_order(event.cl_ord_id)
        if order is None:
            self._log.warning(f"Cannot apply event {event} to any order, "
                              f"{event.cl_ord_id.to_string(with_class=True)} "
                              f"not found in cache.")
            return  # Cannot process event further

        try:
            order.apply(event)
        except InvalidStateTrigger as ex:
            self._log.exception(ex)

        self.database.update_order(order)

        if isinstance(event, OrderFillEvent):
            self._handle_order_fill(event)
            return

        self._send_to_strategy(event, self.database.get_strategy_for_order(event.cl_ord_id))

    cdef void _handle_order_fill(self, OrderFillEvent event) except *:
        cdef ClientPositionId position_id = self.database.get_client_position_id(event.cl_ord_id)
        if position_id is None:
            position_id = self.database.get_client_position_id_for_id(event.position_id)

        if position_id is None:
            self._log.error(f"Cannot process event {event}, "
                            f"PositionId for {event.cl_ord_id.to_string(with_class=True)} "
                            f"not found.")
            return  # Cannot process event further

        cdef Position position = self.database.get_position_for_order(event.cl_ord_id)
        cdef StrategyId strategy_id = self.database.get_strategy_for_position(position_id)
        # position could still be None here

        if strategy_id is None:
            self._log.error(f"Cannot process event {event}, "
                            f"StrategyId for {position_id.to_string(with_class=True)} "
                            f"not found.")
            return  # Cannot process event further

        if position is None:
            # Position does not exist - create new position
            position = Position(position_id, event)
            self.database.add_position(position, strategy_id)
            self._position_opened(position, strategy_id, event)
        else:
            # Position exists - apply event
            position.apply(event)
            self.database.update_position(position)
            if position.is_closed():
                self._position_closed(position, strategy_id, event)
            else:
                self._position_modified(position, strategy_id, event)

    cdef void _handle_position_event(self, PositionEvent event) except *:
        self.portfolio.update(event)
        self._send_to_strategy(event, event.strategy_id)

    cdef void _handle_account_event(self, AccountState event) except *:
        cdef Account account = self.database.get_account(event.account_id)
        if account is None:
            account = Account(event)
            if self.account_id.equals(account.id):
                self.account = account
                self.database.add_account(self.account)
                self.portfolio.set_base_currency(event.currency)
                return
        elif account.id == event.account_id:
            account.apply(event)
            self.database.update_account(account)
            return

        self._log.warning(f"Cannot process event {event}, "
                          f"event {event.account_id.to_string(with_class=True)} "
                          f"does not match traders {self.account_id.to_string(with_class=True)}.")

    cdef void _position_opened(self, Position position, StrategyId strategy_id, OrderEvent event) except *:
        cdef PositionOpened position_opened = PositionOpened(
            position,
            strategy_id,
            event,
            self._uuid_factory.generate(),
            event.timestamp)

        self._send_to_strategy(event, strategy_id)
        self.handle_event(position_opened)

    cdef void _position_modified(self, Position position, StrategyId strategy_id, OrderEvent event) except *:
        cdef PositionModified position_modified = PositionModified(
            position,
            strategy_id,
            event,
            self._uuid_factory.generate(),
            event.timestamp)

        self._send_to_strategy(event, strategy_id)
        self.handle_event(position_modified)

    cdef void _position_closed(self, Position position, StrategyId strategy_id, OrderEvent event) except *:
        cdef datetime time_now = self._clock.utc_now()
        cdef PositionClosed position_closed = PositionClosed(
            position,
            strategy_id,
            event,
            self._uuid_factory.generate(),
            event.timestamp)

        self._send_to_strategy(event, strategy_id)
        self.handle_event(position_closed)

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
        self._registered_strategies = {}  # type: {StrategyId, TradingStrategy}
        self.command_count = 0
        self.event_count = 0
