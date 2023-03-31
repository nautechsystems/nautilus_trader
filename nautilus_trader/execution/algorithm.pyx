# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Any, Optional

from nautilus_trader.config import ExecAlgorithmConfig
from nautilus_trader.config import ImportableExecAlgorithmConfig

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport SENT
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.execution.messages cimport TradingCommand
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ExecAlgorithmId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.list cimport OrderList
from nautilus_trader.msgbus.bus cimport MessageBus
from nautilus_trader.portfolio.base cimport PortfolioFacade


cdef class ExecAlgorithm(Actor):
    """
    The base class for all execution algorithms.

    This class allows traders to implement their own customized execution algorithms.

    Parameters
    ----------
    config : ExecAlgorithmConfig, optional
        The execution algorithm configuration.

    Raises
    ------
    TypeError
        If `config` is not of type `ExecAlgorithmConfig`.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, config: Optional[ExecAlgorithmConfig] = None):
        if config is None:
            config = ExecAlgorithmConfig()
        Condition.type(config, ExecAlgorithmConfig, "config")

        super().__init__()
        # Assign Execution Algorithm ID after base class initialized
        component_id = type(self).__name__ if config.exec_algorithm_id is None else config.exec_algorithm_id
        self.id = ExecAlgorithmId(component_id)

        # Configuration
        self.config = config

        # Public components
        self.portfolio = None  # Initialized when registered

    def to_importable_config(self) -> ImportableExecAlgorithmConfig:
        """
        Returns an importable configuration for this execution algorithm.

        Returns
        -------
        ImportableExecAlgorithmConfig

        """
        return ImportableExecAlgorithmConfig(
            exec_algorithm_path=self.fully_qualified_name(),
            config_path=self.config.fully_qualified_name(),
            config=self.config.dict(),
        )

# -- REGISTRATION ---------------------------------------------------------------------------------

    cpdef void register(
        self,
        TraderId trader_id,
        PortfolioFacade portfolio,
        MessageBus msgbus,
        CacheFacade cache,
        Clock clock,
        Logger logger,
    ):
        """
        Register the execution algorithm with a trader.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID for the execution algorithm.
        portfolio : PortfolioFacade
            The read-only portfolio for the execution algorithm.
        msgbus : MessageBus
            The message bus for the execution algorithm.
        cache : CacheFacade
            The read-only cache for the execution algorithm.
        clock : Clock
            The clock for the execution algorithm.
        logger : Logger
            The logger for the execution algorithm.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(trader_id, "trader_id")
        Condition.not_none(portfolio, "portfolio")
        Condition.not_none(msgbus, "msgbus")
        Condition.not_none(cache, "cache")
        Condition.not_none(clock, "clock")
        Condition.not_none(logger, "logger")

        self.register_base(
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        self.portfolio = portfolio

    cpdef void handle_submit_order(self, SubmitOrder command):
        """
        Handle the give submit order command by processing it with the execution algorithm.

        Parameters
        ----------
        command : SubmitOrder
            The command to handle.

        Raises
        ------
        ValueError
            If `command.exec_algorithm_spec` is ``None``.
        ValueError
            If `command.exec_algorithm_spec.exec_algorithm_id` is not equal to `self.exec_algorithm_id`

        """
        Condition.not_none(command, "command")
        Condition.not_none(command.exec_algorithm_spec, "command.exec_algorithm_spec")
        Condition.equal(
            command.exec_algorithm_spec.exec_algorithm_id,
            self.exec_algorithm_id,
            "command.exec_algorithm_spec.exec_algorithm_id",
            "self.exec_algorithm_id",
        )

    cpdef void handle_submit_order_list(self, SubmitOrderList command):
        """
        Handle the give submit order list command by processing it with the execution algorithm.

        Parameters
        ----------
        command : SubmitOrderList
            The command to handle.

        Raises
        ------
        ValueError
            If `command.exec_algorithm_specs` is empty.
        ValueError
            If the first element of `command.exec_algorithm_specs` `exec_algorithm_id` is not equal to `self.exec_algorith_id`.

        """
        Condition.not_none(command, "command")
        Condition.not_empty(command.exec_algorithm_specs, "command.exec_algorithm_specs")
        Condition.equal(
            command.exec_algorithm_specs[0].exec_algorithm_id,
            self.exec_algorithm_id,
            "command.exec_algorithm_specs[0].exec_algorithm_id",
            "self.exec_algorithm_id",
        )

    cpdef void submit_order(
        self,
        Order order,
        PositionId position_id = None,
        ClientId client_id = None,
    ):
        """
        Submit the given order with optional position ID and routing instructions.

        A `SubmitOrder` command will be created and sent directly to the `ExecutionEngine`.

        If the client order ID is duplicate, then the order will be denied.

        Parameters
        ----------
        order : Order
            The order to submit.
        position_id : PositionId, optional
            The position ID to submit the order against. If a position does not
            yet exist, then any position opened will have this identifier assigned.
        client_id : ClientId, optional
            The specific execution client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        Raises
        ------
        ValueError
            If `order.status` is not ``INITIALIZED``.

        Warning
        -------
        If a `position_id` is passed and a position does not yet exist, then any
        position opened by the order will have this position ID assigned. This may
        not be what you intended.

        """
        Condition.not_none(order, "order")

    cpdef void submit_order_list(
        self,
        OrderList order_list,
        PositionId position_id = None,
        ClientId client_id = None
    ):
        """
        Submit the given order list with optional position ID, execution and routing instructions.

        A `SubmitOrderList` command with be created and sent **directly** to the `ExecutionEngine`.

        If the order list ID is duplicate, or any client order ID is duplicate,
        then all orders will be denied.

        Parameters
        ----------
        order_list : OrderList
            The order list to submit.
        position_id : PositionId, optional
            The position ID to submit the order against. If a position does not
            yet exist, then any position opened will have this identifier assigned.
        client_id : ClientId, optional
            The specific execution client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        Raises
        ------
        ValueError
            If any `order.status` is not ``INITIALIZED``.

        Warning
        -------
        If a `position_id` is passed and a position does not yet exist, then any
        position opened by an order will have this position ID assigned. This may
        not be what you intended.

        """
        Condition.not_none(order_list, "order_list")

    cdef void _send_exec_command(self, TradingCommand command):
        if not self.log.is_bypassed:
            self.log.info(f"{CMD}{SENT} {command}.")
        self._msgbus.send(endpoint="ExecEngine.execute", msg=command)
