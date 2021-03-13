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
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.events cimport OrderDenied
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
            The execution engine for the risk engine.
        portfolio : Portfolio
            The portfolio for the risk engine.
        clock : Clock
            The clock for the risk engine.
        logger : Logger
            The logger for the risk engine.
        config : dict[str, object], optional
            The configuration options.

        """
        if config is None:
            config = {}
        super().__init__(clock, logger, name="RiskEngine")

        self._portfolio = portfolio
        self._exec_engine = exec_engine

        self.block_all_orders = False

        # Check portfolio matches execution engines portfolio
        self._exec_engine.check_portfolio_equal(portfolio)

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

    cpdef void approve_order(self, SubmitOrder command) except *:
        """
        Approve the given command based on risk.

        Parameters
        ----------
        command : SubmitOrder
            The command to approve.

        """
        Condition.not_none(command, "command")

        cdef list risk_msgs = self._check_submit_order_risk(command)

        if self.block_all_orders:
            # TODO: Should potentially still allow 'reduce_only' orders??
            risk_msgs.append("all orders blocked")

        if risk_msgs:
            self._deny_order(command.order, ",".join(risk_msgs))
        else:
            command.approve()
            self._exec_engine.execute(command)

    cpdef void approve_bracket(self, SubmitBracketOrder command) except *:
        """
        Approve the given command based on risk.

        Parameters
        ----------
        command : SubmitBracketOrder
            The command to approve.

        """
        Condition.not_none(command, "command")

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
            command.approve()
            self._exec_engine.execute(command)

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
