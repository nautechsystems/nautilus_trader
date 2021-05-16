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
The `RiskEngine` is responsible for global strategy and portfolio risk within the platform.

Alternative implementations can be written on top of the generic engine.
"""

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport EVT
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Command
from nautilus_trader.core.message cimport Event
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.commands cimport TradingCommand
from nautilus_trader.model.commands cimport UpdateOrder
from nautilus_trader.model.events cimport OrderDenied
from nautilus_trader.model.events cimport OrderInvalid
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.bracket cimport BracketOrder
from nautilus_trader.trading.portfolio cimport Portfolio


cdef class RiskEngine(Component):
    """
    Provides a high-performance risk engine.
    """

    def __init__(
        self,
        ExecutionEngine exec_engine not None,
        Portfolio portfolio not None,
        Clock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the `RiskEngine` class.

        Parameters
        ----------
        exec_engine : ExecutionEngine
            The execution engine for the engine.
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
        super().__init__(clock, logger, name="RiskEngine")

        if config:
            self._log.info(f"Config: {config}.")

        self._portfolio = portfolio
        self._exec_engine = exec_engine

        self.trader_id = exec_engine.trader_id
        self.cache = exec_engine.cache

        self.block_all_orders = False

        # Counters
        self.command_count = 0
        self.event_count = 0

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

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void _on_start(self) except *:
        pass  # Optionally override in subclass

    cpdef void _on_stop(self) except *:
        pass  # Optionally override in subclass

# -- ACTION IMPLEMENTATIONS ------------------------------------------------------------------------

    cpdef void _start(self) except *:
        # Do nothing else for now
        self._on_start()

    cpdef void _stop(self) except *:
        # Do nothing else for now
        self._on_stop()

    cpdef void _reset(self) except *:
        self.command_count = 0
        self.event_count = 0

    cpdef void _dispose(self) except *:
        pass
        # Nothing to dispose for now

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cdef inline void _execute_command(self, Command command) except *:
        self._log.debug(f"{RECV}{CMD} {command}.")
        self.command_count += 1

        if isinstance(command, TradingCommand):
            self._handle_trading_command(command)

    cdef inline void _handle_trading_command(self, TradingCommand command) except *:
        if isinstance(command, SubmitOrder):
            self._handle_submit_order(command)
        elif isinstance(command, SubmitBracketOrder):
            self._handle_submit_bracket_order(command)
        elif isinstance(command, UpdateOrder):
            self._handle_update_order(command)
        elif isinstance(command, CancelOrder):
            self._handle_cancel_order(command)
        else:
            self._log.error(f"Cannot handle command: unrecognized {command}.")

    cdef inline void _handle_submit_order(self, SubmitOrder command) except *:
        # Validate command
        if self.cache.order_exists(command.order.client_order_id):
            self._log.error(f"Cannot submit order: "
                            f"{repr(command.order.client_order_id)} already exists.")
            return  # Invalid command

        # Cache order
        # *** Do not complete additional risk checks before here ***
        self.cache.add_order(command.order, command.position_id)

        if command.position_id.not_null() and not self.cache.position_exists(command.position_id):
            self._invalidate_order(
                command.order.client_order_id,
                f"{repr(command.position_id)} does not exist",
            )
            return  # Invalid command

        cdef list risk_msgs = self._check_submit_order_risk(command)

        if self.block_all_orders:
            # TODO: Should potentially still allow 'reduce_only' orders??
            risk_msgs.append("all orders blocked")

        if risk_msgs:
            self._deny_order(command.order, ",".join(risk_msgs))
            return  # Order denied

        self._exec_engine.execute(command)

    cdef inline void _handle_submit_bracket_order(self, SubmitBracketOrder command) except *:
        # Validate command
        if self.cache.order_exists(command.bracket_order.entry.client_order_id):
            self._invalidate_bracket_order(command.bracket_order)
            return  # Invalid command
        if self.cache.order_exists(command.bracket_order.stop_loss.client_order_id):
            self._invalidate_bracket_order(command.bracket_order)
            return  # Invalid command
        if command.bracket_order.take_profit is not None \
                and self.cache.order_exists(command.bracket_order.take_profit.client_order_id):
            self._invalidate_bracket_order(command.bracket_order)
            return  # Invalid command

        # Cache all orders
        # *** Do not complete additional risk checks before here ***
        self.cache.add_order(command.bracket_order.entry, PositionId.null_c())
        self.cache.add_order(command.bracket_order.stop_loss, PositionId.null_c())
        if command.bracket_order.take_profit is not None:
            self.cache.add_order(command.bracket_order.take_profit, PositionId.null_c())

        cdef list risk_msgs = self._check_submit_bracket_order_risk(command)

        if self.block_all_orders:
            # TODO: Should potentially still allow 'reduce_only' orders??
            risk_msgs.append("all orders blocked")

        if risk_msgs:
            self._deny_order(command.bracket_order.entry, ",".join(risk_msgs))
            self._deny_order(command.bracket_order.stop_loss, ",".join(risk_msgs))
            self._deny_order(command.bracket_order.take_profit, ",".join(risk_msgs))
            return  # Orders denied

        self._exec_engine.execute(command)

    cdef inline void _handle_update_order(self, UpdateOrder command) except *:
        # Validate command
        if self.cache.is_order_completed(command.client_order_id):
            self._log.warning(f"Cannot update order: "
                              f"{repr(command.client_order_id)} already completed.")
            return  # Invalid command

        self._exec_engine.execute(command)

    cdef inline void _handle_cancel_order(self, CancelOrder command) except *:
        # Validate command
        if self.cache.is_order_completed(command.client_order_id):
            self._log.warning(f"Cannot cancel order: "
                              f"{repr(command.client_order_id)} already completed.")
            return  # Invalid command

        self._exec_engine.execute(command)

    cdef inline void _invalidate_order(self, ClientOrderId client_order_id, str reason) except *:
        # Generate event
        cdef OrderInvalid invalid = OrderInvalid(
            client_order_id,
            reason,
            self._uuid_factory.generate(),
            self._clock.timestamp_ns(),
        )

        self._exec_engine.process(invalid)

    cdef inline void _invalidate_bracket_order(self, BracketOrder bracket_order) except *:
        cdef ClientOrderId entry_id = bracket_order.entry.client_order_id
        cdef ClientOrderId stop_loss_id = bracket_order.stop_loss.client_order_id
        cdef ClientOrderId take_profit_id = None
        if bracket_order.take_profit:
            take_profit_id = bracket_order.take_profit.client_order_id

        cdef list error_msgs = []

        # Check entry ----------------------------------------------------------
        if self.cache.order_exists(entry_id):
            error_msgs.append(f"Duplicate {repr(entry_id)}")
        else:
            # Add to cache to be able to invalidate
            self.cache.add_order(bracket_order.entry, PositionId.null_c())
            self._invalidate_order(
                bracket_order.entry.client_order_id,
                "Duplicate ClientOrderId in bracket.",
            )
        # Check stop-loss ------------------------------------------------------
        if self.cache.order_exists(stop_loss_id):
            error_msgs.append(f"Duplicate {repr(stop_loss_id)}")
        else:
            # Add to cache to be able to invalidate
            self.cache.add_order(bracket_order.stop_loss, PositionId.null_c())
            self._invalidate_order(
                bracket_order.stop_loss.client_order_id,
                "Duplicate ClientOrderId in bracket.",
            )
        # Check take-profit ----------------------------------------------------
        if take_profit_id is not None and self.cache.order_exists(take_profit_id):
            error_msgs.append(f"Duplicate {repr(take_profit_id)}")
        else:
            # Add to cache to be able to invalidate
            self.cache.add_order(bracket_order.take_profit, PositionId.null_c())
            self._invalidate_order(
                bracket_order.take_profit.client_order_id,
                "Duplicate ClientOrderId in bracket.",
            )

        # Finally log error
        self._log.error(f"Cannot submit BracketOrder: {', '.join(error_msgs)}")

# -- EVENT HANDLERS --------------------------------------------------------------------------------

    cdef inline void _handle_event(self, Event event) except *:
        self._log.debug(f"{RECV}{EVT} {event}.")
        self.event_count += 1

# -- RISK MANAGEMENT -------------------------------------------------------------------------------

    cdef list _check_submit_order_risk(self, SubmitOrder command):
        # Override this implementation with custom logic
        return []

    cdef list _check_submit_bracket_order_risk(self, SubmitBracketOrder command):
        # Override this implementation with custom logic
        return []

    cdef void _deny_order(self, Order order, str reason) except *:
        # Generate event
        cdef OrderDenied denied = OrderDenied(
            client_order_id=order.client_order_id,
            reason=reason,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._exec_engine.process(denied)

# -- TEMP ------------------------------------------------------------------------------------------

    cpdef void set_block_all_orders(self, bint value=True) except *:
        """
        Set the global `block_all_orders` flag to the given value.

        Parameters
        ----------
        value : bool
            The flag setting.

        """
        self.block_all_orders = value
        self._log.warning(f"`block_all_orders` set to {value}.")
