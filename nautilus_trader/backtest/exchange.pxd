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

from nautilus_trader.backtest.execution cimport BacktestExecClient
from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.backtest.modules cimport SimulationModule
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.execution.cache cimport ExecutionCache
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.oms_type cimport OMSType
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport ModifyOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.order cimport LimitOrder
from nautilus_trader.model.order cimport MarketOrder
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.order cimport PassiveOrder
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.trading.account cimport Account
from nautilus_trader.trading.calculators cimport ExchangeRateCalculator


cdef class SimulatedExchange:
    cdef Clock _clock
    cdef UUIDFactory _uuid_factory
    cdef LoggerAdapter _log

    cdef readonly Venue venue
    cdef readonly OMSType oms_type
    cdef readonly ExecutionCache exec_cache
    cdef readonly BacktestExecClient exec_client
    cdef readonly Account account
    cdef readonly Currency account_currency
    cdef readonly Money starting_capital
    cdef readonly Money account_balance
    cdef readonly Money account_start_day
    cdef readonly Money account_activity_day
    cdef readonly Money total_commissions
    cdef readonly bint frozen_account
    cdef readonly bint generate_position_ids

    cdef readonly ExchangeRateCalculator xrate_calculator
    cdef readonly FillModel fill_model
    cdef readonly list modules

    cdef readonly dict instruments
    cdef readonly dict data_ticks
    cdef dict _market_bids
    cdef dict _market_asks
    cdef dict _slippages

    cdef dict _working_orders
    cdef dict _position_index
    cdef dict _child_orders
    cdef dict _oco_orders
    cdef dict _position_oco_orders
    cdef dict _symbol_pos_count
    cdef dict _symbol_ord_count
    cdef int _executions_count

    cpdef void register_client(self, BacktestExecClient client) except *
    cpdef void load_module(self, SimulationModule module) except *
    cpdef void check_residuals(self) except *
    cpdef void reset(self) except *
    cpdef datetime time_now(self)
    cpdef void change_fill_model(self, FillModel fill_model) except *
    cpdef void process_tick(self, QuoteTick tick) except *

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cpdef void handle_submit_order(self, SubmitOrder command) except *
    cpdef void handle_submit_bracket_order(self, SubmitBracketOrder command) except *
    cpdef void handle_modify_order(self, ModifyOrder command) except *
    cpdef void handle_cancel_order(self, CancelOrder command) except *

# --------------------------------------------------------------------------------------------------

    cpdef void adjust_account(self, Money adjustment) except *

    cdef inline Price get_current_bid(self, Symbol symbol)
    cdef inline Price get_current_ask(self, Symbol symbol)
    cdef inline object get_xrate(self, Currency from_currency, Currency to_currency, PriceType price_type)
    cdef inline dict _build_current_bid_rates(self)
    cdef inline dict _build_current_ask_rates(self)

# -- EVENT HANDLING --------------------------------------------------------------------------------

    cdef inline object _get_tick_sizes(self)
    cdef inline PositionId _generate_position_id(self, Symbol symbol)
    cdef inline OrderId _generate_order_id(self, Symbol symbol)
    cdef inline ExecutionId _generate_execution_id(self)
    cdef inline AccountState _generate_account_event(self)
    cdef inline void _submit_order(self, Order order) except *
    cdef inline void _accept_order(self, Order order) except *
    cdef inline void _reject_order(self, Order order, str reason) except *
    cdef inline void _cancel_reject_order(self, ClientOrderId order_id, str response, str reason) except *
    cdef inline void _expire_order(self, PassiveOrder order) except *
    cdef inline void _process_order(self, Order order) except *
    cdef inline void _process_market_order(self, MarketOrder order, Price market_bid, Price market_ask) except *
    cdef inline void _process_limit_order(self, LimitOrder order, Price market_bid, Price market_ask) except *
    cdef inline void _process_passive_order(self, PassiveOrder order, Price market_bid, Price market_ask) except *
    cdef inline void _work_order(self, Order order) except *
    cdef inline void _auction_buy_order(self, PassiveOrder order, Price market) except *
    cdef inline void _auction_buy_stop_order(self, PassiveOrder order, Price market) except *
    cdef inline void _auction_buy_limit_order(self, PassiveOrder order, Price market) except *
    cdef inline void _auction_sell_order(self, PassiveOrder order, Price market) except *
    cdef inline void _auction_sell_stop_order(self, PassiveOrder order, Price market) except *
    cdef inline void _auction_sell_limit_order(self, PassiveOrder order, Price market) except *
    cdef inline bint _is_marginal_limit_fill(self, Price order_price, Price market) except *
    cdef inline bint _is_marginal_stop_fill(self, Price order_price, Price market) except *
    cdef inline void _fill_order(self, Order order, Price fill_price, LiquiditySide liquidity_side) except *
    cdef inline void _clean_up_child_orders(self, ClientOrderId order_id) except *
    cdef inline void _check_oco_order(self, ClientOrderId order_id) except *
    cdef inline void _reject_oco_order(self, PassiveOrder order, ClientOrderId oco_order_id) except *
    cdef inline void _cancel_oco_order(self, PassiveOrder order, ClientOrderId oco_order_id) except *
    cdef inline void _cancel_order(self, PassiveOrder order) except *
