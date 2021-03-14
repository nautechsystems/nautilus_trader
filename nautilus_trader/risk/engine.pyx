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
from nautilus_trader.execution.client cimport ExecutionClient
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.commands cimport AmendOrder
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.commands cimport TradingCommand
from nautilus_trader.model.events cimport OrderDenied
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.order.base cimport Order
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

        self._clients = {}  # type: dict[Venue, ExecutionClient]
        self._portfolio = portfolio
        self._exec_engine = exec_engine

        self.block_all_orders = False

        # Counters
        self.command_count = 0
        self.event_count = 0

    @property
    def registered_clients(self):
        """
        The execution clients registered with the engine.

        Returns
        -------
        list[Venue]

        """
        return sorted(list(self._clients.keys()))

# -- REGISTRATION ----------------------------------------------------------------------------------

    cpdef void register_client(self, ExecutionClient client) except *:
        """
        Register the given execution client with the risk engine.

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
        cdef ExecutionClient client = self._clients.get(command.venue)
        if client is None:
            self._log.error(f"Cannot handle command: "
                            f"No client registered for {command.venue}, {command}.")
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
            self._log.error(f"Cannot handle command: unrecognized {command}.")

    cdef inline void _handle_submit_order(self, ExecutionClient client, SubmitOrder command) except *:
        cdef list risk_msgs = self._check_submit_order_risk(command)

        if self.block_all_orders:
            # TODO: Should potentially still allow 'reduce_only' orders??
            risk_msgs.append("all orders blocked")

        if risk_msgs:
            self._deny_order(command.order, ",".join(risk_msgs))
        else:
            client.submit_order(command)

    cdef inline void _handle_submit_bracket_order(self, ExecutionClient client, SubmitBracketOrder command) except *:
        # TODO: Below currently just cut-and-pasted from above. Can refactor further.
        cdef list risk_msgs = self._check_submit_bracket_order_risk(command)

        if self.block_all_orders:
            # TODO: Should potentially still allow 'reduce_only' orders??
            risk_msgs.append("all orders blocked")

        if risk_msgs:
            self._deny_order(command.bracket_order.entry, ",".join(risk_msgs))
            self._deny_order(command.bracket_order.stop_loss, ",".join(risk_msgs))
            self._deny_order(command.bracket_order.take_profit, ",".join(risk_msgs))
        else:
            client.submit_bracket_order(command)

    cdef inline void _handle_amend_order(self, ExecutionClient client, AmendOrder command) except *:
        # Pass-through for now
        client.amend_order(command)

    cdef inline void _handle_cancel_order(self, ExecutionClient client, CancelOrder command) except *:
        # Pass-through for now
        client.cancel_order(command)

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
            cl_ord_id=order.cl_ord_id,
            reason=reason,
            event_id=self._uuid_factory.generate_c(),
            event_timestamp=self._clock.utc_now_c(),
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
