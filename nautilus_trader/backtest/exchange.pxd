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

from libc.stdint cimport int64_t

from nautilus_trader.backtest.execution cimport BacktestExecClient
from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.execution.cache cimport ExecutionCache
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.oms_type cimport OMSType
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.commands cimport UpdateOrder
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order.base cimport Order
from nautilus_trader.model.order.base cimport PassiveOrder
from nautilus_trader.model.order.limit cimport LimitOrder
from nautilus_trader.model.order.market cimport MarketOrder
from nautilus_trader.model.order.stop_limit cimport StopLimitOrder
from nautilus_trader.model.order.stop_market cimport StopMarketOrder
from nautilus_trader.model.orderbook.book cimport OrderBookData
from nautilus_trader.model.tick cimport Tick
from nautilus_trader.trading.calculators cimport ExchangeRateCalculator


cdef class SimulatedExchange:
    cdef Clock _clock
    cdef UUIDFactory _uuid_factory
    cdef LoggerAdapter _log

    cdef readonly Venue id
    cdef readonly OMSType oms_type
    cdef readonly bint generate_position_ids

    cdef readonly ExecutionCache exec_cache
    cdef readonly BacktestExecClient exec_client

    cdef readonly bint is_frozen_account
    cdef readonly list starting_balances
    cdef readonly Currency default_currency
    cdef readonly dict account_balances
    cdef readonly dict account_balances_free
    cdef readonly dict account_balances_locked
    cdef readonly dict total_commissions

    cdef readonly ExchangeRateCalculator xrate_calculator
    cdef readonly FillModel fill_model
    cdef readonly list modules

    cdef readonly dict instruments
    cdef readonly dict data_ticks

    cdef dict _books
    cdef dict _market_bids
    cdef dict _market_asks
    cdef dict _slippages

    cdef dict _working_orders
    cdef dict _position_index
    cdef dict _child_orders
    cdef dict _oco_orders
    cdef dict _position_oco_orders
    cdef dict _instrument_indexer
    cdef dict _symbol_pos_count
    cdef dict _symbol_ord_count
    cdef int _executions_count

    cpdef dict get_working_orders(self)
    cpdef void register_client(self, BacktestExecClient client) except *
    cpdef void set_fill_model(self, FillModel fill_model) except *
    cpdef void initialize_account(self) except *
    cpdef void process_order_book(self, OrderBookData data) except *
    cpdef void process_tick(self, Tick tick) except *
    cpdef void process_modules(self, int64_t now_ns) except *
    cpdef void check_residuals(self) except *
    cpdef void reset(self) except *

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cpdef void handle_submit_order(self, SubmitOrder command) except *
    cpdef void handle_submit_bracket_order(self, SubmitBracketOrder command) except *
    cpdef void handle_update_order(self, UpdateOrder command) except *
    cpdef void handle_cancel_order(self, CancelOrder command) except *

# --------------------------------------------------------------------------------------------------

    cpdef void adjust_account(self, Money adjustment) except *

    cdef inline Price get_current_bid(self, InstrumentId instrument_id)
    cdef inline Price get_current_ask(self, InstrumentId instrument_id)
    cdef inline object get_xrate(self, Currency from_currency, Currency to_currency, PriceType price_type)
    cdef inline dict _build_current_bid_rates(self)
    cdef inline dict _build_current_ask_rates(self)

# -- EVENT HANDLING --------------------------------------------------------------------------------

    cdef inline object _get_tick_sizes(self)
    cdef inline PositionId _generate_position_id(self, InstrumentId instrument_id)
    cdef inline VenueOrderId _generate_order_id(self, InstrumentId instrument_id)
    cdef inline ExecutionId _generate_execution_id(self)
    cdef inline AccountState _generate_account_event(self)
    cdef inline void _submit_order(self, Order order) except *
    cdef inline void _accept_order(self, Order order) except *
    cdef inline void _reject_order(self, Order order, str reason) except *
    cdef inline void _update_order(self, ClientOrderId client_order_id, Quantity qty, Price price) except *
    cdef inline void _cancel_order(self, ClientOrderId client_order_id) except *
    cdef inline void _reject_cancel(self, ClientOrderId client_order_id, str response, str reason) except *
    cdef inline void _reject_update(self, ClientOrderId client_order_id, str response, str reason) except *
    cdef inline void _expire_order(self, PassiveOrder order) except *
    cdef inline void _trigger_order(self, StopLimitOrder order) except *
    cdef inline void _process_order(self, Order order) except *
    cdef inline void _process_market_order(self, MarketOrder order, Price bid, Price ask) except *
    cdef inline void _process_limit_order(self, LimitOrder order, Price bid, Price ask) except *
    cdef inline void _process_stop_market_order(self, StopMarketOrder order, Price bid, Price ask) except *
    cdef inline void _process_stop_limit_order(self, StopLimitOrder order, Price bid, Price ask) except *
    cdef inline void _update_limit_order(self, LimitOrder order, Quantity qty, Price price, Price bid, Price ask) except *
    cdef inline void _update_stop_market_order(self, StopMarketOrder order, Quantity qty, Price price, Price bid, Price ask) except *
    cdef inline void _update_stop_limit_order(self, StopLimitOrder order, Quantity qty, Price price, Price bid, Price ask) except *
    cdef inline void _generate_order_updated(self, PassiveOrder order, Quantity qty, Price price) except *

# -- ORDER MATCHING ENGINE -------------------------------------------------------------------------

    cdef inline void _iterate_matching_engine(self, InstrumentId instrument_id, Price bid, Price ask, int64_t timestamp_ns) except *
    cdef inline void _match_order(self, PassiveOrder order, Price bid, Price ask) except *
    cdef inline void _match_limit_order(self, LimitOrder order, Price bid, Price ask) except *
    cdef inline void _match_stop_market_order(self, StopMarketOrder order, Price bid, Price ask) except *
    cdef inline void _match_stop_limit_order(self, StopLimitOrder order, Price bid, Price ask) except *
    cdef inline bint _is_limit_marketable(self, OrderSide side, Price order_price, Price bid, Price ask) except *
    cdef inline bint _is_limit_matched(self, OrderSide side, Price order_price, Price bid, Price ask) except *
    cdef inline bint _is_stop_marketable(self, OrderSide side, Price order_price, Price bid, Price ask) except *
    cdef inline bint _is_stop_triggered(self, OrderSide side, Price order_price, Price bid, Price ask) except *
    cdef inline Price _fill_price_maker(self, OrderSide side, Price bid, Price ask)
    cdef inline Price _fill_price_taker(self, InstrumentId instrument_id, OrderSide side, Price bid, Price ask)
    cdef inline Price _fill_price_stop(self, InstrumentId instrument_id, OrderSide side, Price stop)

# --------------------------------------------------------------------------------------------------

    cdef inline void _fill_order(self, Order order, Price fill_px, LiquiditySide liquidity_side) except *
    cdef inline void _clean_up_child_orders(self, ClientOrderId client_order_id) except *
    cdef inline void _check_oco_order(self, ClientOrderId client_order_id) except *
    cdef inline void _reject_oco_order(self, PassiveOrder order, ClientOrderId other_oco) except *
    cdef inline void _cancel_oco_order(self, PassiveOrder order) except *
