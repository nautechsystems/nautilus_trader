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
from nautilus_trader.common.factories cimport OrderFactory
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.timer cimport TimeEvent
from nautilus_trader.execution.algorithm cimport ExecAlgorithmSpecification
from nautilus_trader.execution.messages cimport TradingCommand
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.enums_c cimport OmsType
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.enums_c cimport PositionSide
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.list cimport OrderList
from nautilus_trader.model.position cimport Position
from nautilus_trader.msgbus.bus cimport MessageBus
from nautilus_trader.portfolio.base cimport PortfolioFacade


cdef class Strategy(Actor):
    cdef list _indicators
    cdef dict _indicators_for_quotes
    cdef dict _indicators_for_trades
    cdef dict _indicators_for_bars
    cdef bint _manage_gtd_expiry

    cdef readonly PortfolioFacade portfolio
    """The read-only portfolio for the strategy.\n\n:returns: `PortfolioFacade`"""
    cdef readonly OrderFactory order_factory
    """The order factory for the strategy.\n\n:returns: `OrderFactory`"""
    cdef readonly OmsType oms_type
    """The order management system for the strategy.\n\n:returns: `OmsType`"""
    cdef readonly str order_id_tag
    """The order ID tag for the strategy.\n\n:returns: `str`"""

    cpdef bint indicators_initialized(self) except *

# -- REGISTRATION ---------------------------------------------------------------------------------

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

# -- TRADING COMMANDS -----------------------------------------------------------------------------

    cpdef void submit_order(
        self,
        Order order,
        PositionId position_id=*,
        bint manage_gtd_expiry=*,
        ExecAlgorithmSpecification exec_algorithm_spec=*,
        ClientId client_id=*,
    ) except *
    cpdef void submit_order_list(
        self,
        OrderList order_list,
        PositionId position_id=*,
        bint manage_gtd_expiry=*,
        list exec_algorithm_specs=*,
        ClientId client_id=*,
    ) except *
    cpdef void modify_order(
        self,
        Order order,
        Quantity quantity=*,
        Price price=*,
        Price trigger_price=*,
        ClientId client_id=*,
    ) except *
    cpdef void cancel_order(self, Order order, ClientId client_id=*) except *
    cpdef void cancel_all_orders(self, InstrumentId instrument_id, OrderSide order_side=*, ClientId client_id=*) except *
    cpdef void close_position(self, Position position, ClientId client_id=*, str tags=*) except *
    cpdef void close_all_positions(self, InstrumentId instrument_id, PositionSide position_side=*, ClientId client_id=*, str tags=*) except *
    cpdef void query_order(self, Order order, ClientId client_id=*) except *

    cdef str _get_gtd_expiry_timer_name(self, ClientOrderId client_order_id)
    cdef void _set_gtd_expiry(self, Order order) except *
    cdef void _cancel_gtd_expiry(self, Order order) except *
    cpdef void _expire_gtd_order(self, TimeEvent event) except *

# -- HANDLERS -------------------------------------------------------------------------------------

    cdef void _handle_indicators_for_quote(self, list indicators, QuoteTick tick) except *
    cdef void _handle_indicators_for_trade(self, list indicators, TradeTick tick) except *
    cdef void _handle_indicators_for_bar(self, list indicators, Bar bar) except *

# -- EGRESS ---------------------------------------------------------------------------------------

    cdef void _send_risk_command(self, TradingCommand command) except *
    cdef void _send_exec_command(self, TradingCommand command) except *
