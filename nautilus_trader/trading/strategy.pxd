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

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.factories cimport OrderFactory
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.fsm cimport FiniteStateMachine
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
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order cimport BracketOrder
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick
from nautilus_trader.trading.portfolio cimport PortfolioFacade


cdef class TradingStrategy:
    cdef StrategyId _id
    cdef TraderId _trader_id
    cdef Clock _clock
    cdef UUIDFactory _uuid_factory
    cdef LoggerAdapter _log
    cdef DataCacheFacade _data
    cdef ExecutionCacheFacade _execution
    cdef PortfolioFacade _portfolio
    cdef OrderFactory _order_factory

    cdef list _indicators
    cdef dict _indicators_for_quotes
    cdef dict _indicators_for_trades
    cdef dict _indicators_for_bars

    cdef DataEngine _data_engine
    cdef ExecutionEngine _exec_engine
    cdef FiniteStateMachine _fsm

    cdef str state_string(self)

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void on_start(self) except *
    cpdef void on_quote_tick(self, QuoteTick tick) except *
    cpdef void on_trade_tick(self, TradeTick tick) except *
    cpdef void on_bar(self, BarType bar_type, Bar bar) except *
    cpdef void on_data(self, object data) except *
    cpdef void on_event(self, Event event) except *
    cpdef void on_stop(self) except *
    cpdef void on_resume(self) except *
    cpdef void on_reset(self) except *
    cpdef dict on_save(self)
    cpdef void on_load(self, dict state) except *
    cpdef void on_dispose(self) except *

# -- REGISTRATION METHODS --------------------------------------------------------------------------

    cpdef void register_trader(
        self,
        TraderId trader_id,
        Clock clock,
        UUIDFactory uuid_factory,
        Logger logger,
    ) except *
    cpdef void register_data_engine(self, DataEngine engine) except *
    cpdef void register_execution_engine(self, ExecutionEngine engine) except *
    cpdef void register_indicator_for_quote_ticks(self, Symbol symbol, Indicator indicator) except *
    cpdef void register_indicator_for_trade_ticks(self, Symbol symbol, Indicator indicator) except *
    cpdef void register_indicator_for_bars(self, BarType bar_type, Indicator indicator) except *

# -- HANDLER METHODS -------------------------------------------------------------------------------

    cpdef void handle_quote_tick(self, QuoteTick tick, bint is_historical=*) except *
    cpdef void handle_quote_ticks(self, list ticks) except *
    cpdef void handle_trade_tick(self, TradeTick tick, bint is_historical=*) except *
    cpdef void handle_trade_ticks(self, list ticks) except *
    cpdef void handle_bar(self, BarType bar_type, Bar bar, bint is_historical=*) except *
    cpdef void handle_bars(self, BarType bar_type, list bars) except *
    cpdef void handle_data(self, object data) except *
    cpdef void handle_event(self, Event event) except *

# -- DATA METHODS ----------------------------------------------------------------------------------

    cpdef void request_quote_ticks(self, Symbol symbol) except *
    cpdef void request_trade_ticks(self, Symbol symbol) except *
    cpdef void request_bars(self, BarType bar_type) except *
    cpdef void subscribe_quote_ticks(self, Symbol symbol) except *
    cpdef void subscribe_trade_ticks(self, Symbol symbol) except *
    cpdef void subscribe_bars(self, BarType bar_type) except *
    cpdef void subscribe_instrument(self, Symbol symbol) except *
    cpdef void unsubscribe_quote_ticks(self, Symbol symbol) except *
    cpdef void unsubscribe_trade_ticks(self, Symbol symbol) except *
    cpdef void unsubscribe_bars(self, BarType bar_type) except *
    cpdef void unsubscribe_instrument(self, Symbol symbol) except *

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void start(self) except *
    cpdef void stop(self) except *
    cpdef void resume(self) except *
    cpdef void reset(self) except *
    cpdef void dispose(self) except *
    cpdef dict save(self)
    cpdef void load(self, dict state) except *
    cpdef void submit_order(self, Order order, PositionId position_id=*) except *
    cpdef void submit_bracket_order(self, BracketOrder bracket_order) except *
    cpdef void modify_order(self, Order order, Quantity new_quantity=*, Price new_price=*) except *
    cpdef void cancel_order(self, Order order) except *
    cpdef void cancel_all_orders(self, Symbol symbol) except *
    cpdef void flatten_position(self, Position position) except *
    cpdef void flatten_all_positions(self, Symbol symbol) except *
