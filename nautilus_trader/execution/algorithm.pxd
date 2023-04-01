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

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.execution.messages cimport ExecAlgorithmSpecification
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.execution.messages cimport TradingCommand
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.list cimport OrderList
from nautilus_trader.msgbus.bus cimport MessageBus
from nautilus_trader.portfolio.base cimport PortfolioFacade


cdef class ExecAlgorithm(Actor):

    cdef readonly PortfolioFacade portfolio
    """The read-only portfolio for the strategy.\n\n:returns: `PortfolioFacade`"""

# -- REGISTRATION ---------------------------------------------------------------------------------

    cpdef void register(
        self,
        TraderId trader_id,
        PortfolioFacade portfolio,
        MessageBus msgbus,
        CacheFacade cache,
        Clock clock,
        Logger logger,
    )

# -- EVENT HANDLERS -------------------------------------------------------------------------------

    cpdef void handle_submit_order(self, SubmitOrder command)
    cpdef void handle_submit_order_list(self, SubmitOrderList command)

    cpdef void on_order(self, Order order, ExecAlgorithmSpecification exec_algorithm_spec)
    cpdef void on_order_list(self, OrderList order_list, list exec_algorithms_specs)

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void submit_order(self, Order order, ClientOrderId parent_order_id=*)
    cpdef void submit_order_list(self, OrderList order_list, PositionId position_id=*)

    cdef void _send_exec_command(self, TradingCommand command)
