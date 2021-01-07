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

from cpython.datetime cimport datetime

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.factories cimport OrderFactory
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.data.cache cimport DataCacheFacade
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.execution.base cimport ExecutionCacheFacade
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.events cimport Event
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order cimport BracketOrder
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.order cimport PassiveOrder
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick
from nautilus_trader.trading.portfolio cimport PortfolioFacade


cdef class TradingStrategy(Component):
    cdef DataEngine _data_engine
    cdef ExecutionEngine _exec_engine
    cdef list _indicators
    cdef dict _indicators_for_quotes
    cdef dict _indicators_for_trades
    cdef dict _indicators_for_bars

    cdef readonly TraderId trader_id
    """The trader identifier associated with the trading strategy.\n\n:returns: `TraderId`"""
    cdef readonly StrategyId id
    """The trading strategies identifier.\n\n:returns: `StrategyId`"""
    cdef readonly Clock clock
    """The trading strategies clock.\n\n:returns: `Clock`"""
    cdef readonly UUIDFactory uuid_factory
    """The trading strategies UUID factory.\n\n:returns: `UUIDFactory`"""
    cdef readonly LoggerAdapter log
    """The trading strategies logger adapter.\n\n:returns: `LoggerAdapter`"""
    cdef readonly DataCacheFacade data
    """The read-only cache of the `DataEngine` the strategy is registered with.\n\n:returns: `DataCacheFacade`"""
    cdef readonly ExecutionCacheFacade execution
    """The read-only cache of the `ExecutionEngine` the strategy is registered with.\n\n:returns: `ExecutionCacheFacade`"""
    cdef readonly PortfolioFacade portfolio
    """The read-only portfolio the trading strategy is registered with.\n\n:returns: `PortfolioFacade`"""
    cdef readonly OrderFactory order_factory
    """The trading strategies order factory.\n\n:returns: `OrderFactory`"""

    cdef inline void _check_trader_registered(self) except *

    cpdef bint indicators_initialized(self) except *

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void on_start(self) except *
    cpdef void on_stop(self) except *
    cpdef void on_resume(self) except *
    cpdef void on_reset(self) except *
    cpdef dict on_save(self)
    cpdef void on_load(self, dict state) except *
    cpdef void on_dispose(self) except *
    cpdef void on_instrument(self, Instrument instrument) except *
    cpdef void on_quote_tick(self, QuoteTick tick) except *
    cpdef void on_trade_tick(self, TradeTick tick) except *
    cpdef void on_bar(self, BarType bar_type, Bar bar) except *
    cpdef void on_data(self, data) except *
    cpdef void on_event(self, Event event) except *

# -- REGISTRATION ----------------------------------------------------------------------------------

    cpdef void register_trader(
        self,
        TraderId trader_id,
        Clock clock,
        Logger logger,
    ) except *
    cpdef void register_data_engine(self, DataEngine engine) except *
    cpdef void register_execution_engine(self, ExecutionEngine engine) except *
    cpdef void register_indicator_for_quote_ticks(self, Symbol symbol, Indicator indicator) except *
    cpdef void register_indicator_for_trade_ticks(self, Symbol symbol, Indicator indicator) except *
    cpdef void register_indicator_for_bars(self, BarType bar_type, Indicator indicator) except *

# -- STRATEGY COMMANDS -----------------------------------------------------------------------------

    cpdef dict save(self)
    cpdef void load(self, dict state) except *

# -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    cpdef void subscribe_instrument(self, Symbol symbol) except *
    cpdef void subscribe_quote_ticks(self, Symbol symbol) except *
    cpdef void subscribe_trade_ticks(self, Symbol symbol) except *
    cpdef void subscribe_bars(self, BarType bar_type) except *
    cpdef void unsubscribe_instrument(self, Symbol symbol) except *
    cpdef void unsubscribe_quote_ticks(self, Symbol symbol) except *
    cpdef void unsubscribe_trade_ticks(self, Symbol symbol) except *
    cpdef void unsubscribe_bars(self, BarType bar_type) except *

# -- REQUESTS --------------------------------------------------------------------------------------

    cpdef void request_quote_ticks(
        self,
        Symbol symbol,
        datetime from_datetime=*,
        datetime to_datetime=*,
    ) except *
    cpdef void request_trade_ticks(
        self,
        Symbol symbol,
        datetime from_datetime=*,
        datetime to_datetime=*,
    ) except *
    cpdef void request_bars(
        self,
        BarType bar_type,
        datetime from_datetime=*,
        datetime to_datetime=*,
    ) except *

# -- TRADING COMMANDS ------------------------------------------------------------------------------

    cpdef void submit_order(self, Order order, PositionId position_id=*) except *
    cpdef void submit_bracket_order(self, BracketOrder bracket_order) except *
    cpdef void modify_order(self, PassiveOrder order, Quantity new_quantity=*, Price new_price=*) except *
    cpdef void cancel_order(self, Order order) except *
    cpdef void cancel_all_orders(self, Symbol symbol) except *
    cpdef void flatten_position(self, Position position) except *
    cpdef void flatten_all_positions(self, Symbol symbol) except *

# -- HANDLERS --------------------------------------------------------------------------------------

    cpdef void handle_instrument(self, Instrument instrument) except *
    cpdef void handle_quote_tick(self, QuoteTick tick, bint is_historical=*) except *
    cpdef void handle_quote_ticks(self, list ticks) except *
    cpdef void handle_trade_tick(self, TradeTick tick, bint is_historical=*) except *
    cpdef void handle_trade_ticks(self, list ticks) except *
    cpdef void handle_bar(self, BarType bar_type, Bar bar, bint is_historical=*) except *
    cpdef void handle_bars(self, BarType bar_type, list bars) except *
    cpdef void handle_data(self, data) except *
    cpdef void handle_event(self, Event event) except *

    cdef void handle_event_c(self, Event event) except *
