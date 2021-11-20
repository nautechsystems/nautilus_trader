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

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.factories cimport OrderFactory
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.c_enums.oms_type cimport OMSType
from nautilus_trader.model.commands.trading cimport TradingCommand
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.base cimport PassiveOrder
from nautilus_trader.model.orders.list cimport OrderList
from nautilus_trader.model.position cimport Position
from nautilus_trader.msgbus.bus cimport MessageBus
from nautilus_trader.portfolio.base cimport PortfolioFacade


cdef class TradingStrategy(Actor):
    cdef list _indicators
    cdef dict _indicators_for_quotes
    cdef dict _indicators_for_trades
    cdef dict _indicators_for_bars

    cdef readonly UUIDFactory uuid_factory
    """The trading strategies UUID4 factory.\n\n:returns: `UUIDFactory`"""
    cdef readonly LoggerAdapter log
    """The trading strategies logger adapter.\n\n:returns: `LoggerAdapter`"""
    cdef readonly PortfolioFacade portfolio
    """The read-only portfolio for the strategy.\n\n:returns: `PortfolioFacade`"""
    cdef readonly OrderFactory order_factory
    """The order factory for the strategy.\n\n:returns: `OrderFactory`"""
    cdef readonly OMSType oms_type
    """The order management system for the strategy.\n\n:returns: `OMSType`"""

    cpdef bint indicators_initialized(self) except *

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef dict on_save(self)
    cpdef void on_load(self, dict state) except *

# -- REGISTRATION ----------------------------------------------------------------------------------

    cpdef void register(
        self,
        TraderId trader_id,
        PortfolioFacade portfolio,
        MessageBus msgbus,
        CacheFacade cache,
        Clock clock,
        Logger logger,
    ) except *
    cpdef void register_indicator_for_quote_ticks(self, InstrumentId instrument_id, Indicator indicator) except *
    cpdef void register_indicator_for_trade_ticks(self, InstrumentId instrument_id, Indicator indicator) except *
    cpdef void register_indicator_for_bars(self, BarType bar_type, Indicator indicator) except *

# -- STRATEGY COMMANDS -----------------------------------------------------------------------------

    cpdef dict save(self)
    cpdef void load(self, dict state) except *

# -- TRADING COMMANDS ------------------------------------------------------------------------------

    cpdef void submit_order(self, Order order, PositionId position_id=*) except *
    cpdef void submit_order_list(self, OrderList order_list) except *
    cpdef void modify_order(
        self,
        PassiveOrder order,
        Quantity quantity=*,
        Price price=*,
        Price trigger=*,
    ) except *
    cpdef void cancel_order(self, Order order) except *
    cpdef void cancel_all_orders(self, InstrumentId instrument_id) except *
    cpdef void flatten_position(self, Position position) except *
    cpdef void flatten_all_positions(self, InstrumentId instrument_id) except *

# -- EGRESS ----------------------------------------------------------------------------------------

    cdef void _send_exec_cmd(self, TradingCommand command) except *
