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

from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.common.account cimport Account
from nautilus_trader.common.brokerage cimport CommissionCalculator
from nautilus_trader.common.brokerage cimport RolloverInterestCalculator
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.exchange cimport ExchangeRateCalculator
from nautilus_trader.common.execution_engine cimport ExecutionEngine
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.market_position cimport MarketPosition
from nautilus_trader.model.events cimport AccountStateEvent
from nautilus_trader.model.events cimport OrderFillEvent
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.order cimport MarketOrder
from nautilus_trader.model.order cimport PassiveOrder
from nautilus_trader.model.order cimport LimitOrder
from nautilus_trader.model.commands cimport AccountInquiry
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport ModifyOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder


cdef class SimulatedBroker:
    cdef Clock _clock
    cdef UUIDFactory _uuid_factory
    cdef LoggerAdapter _log
    cdef Account _account

    cdef readonly ExecutionEngine exec_engine
    cdef readonly dict instruments
    cdef readonly dict data_ticks
    cdef readonly int day_number
    cdef readonly datetime rollover_time
    cdef readonly bint rollover_applied
    cdef readonly bint frozen_account
    cdef readonly Currency account_currency
    cdef readonly Money starting_capital
    cdef readonly Money account_capital
    cdef readonly Money account_cash_start_day
    cdef readonly Money account_cash_activity_day
    cdef readonly ExchangeRateCalculator exchange_calculator
    cdef readonly CommissionCalculator commission_calculator
    cdef readonly RolloverInterestCalculator rollover_calculator
    cdef readonly double rollover_spread
    cdef readonly Money total_commissions
    cdef readonly Money total_rollover
    cdef readonly FillModel fill_model

    cdef dict _market
    cdef dict _slippages
    cdef dict _min_stops
    cdef dict _min_limits

    cdef dict _working_orders
    cdef dict _child_orders
    cdef dict _oco_orders
    cdef dict _position_oco_orders

    cdef void _set_slippages(self) except *
    cdef void _set_min_distances(self) except *
    cdef dict _build_current_bid_rates(self)
    cdef dict _build_current_ask_rates(self)

    cpdef void check_residuals(self) except *
    cpdef void reset(self) except *
    cdef AccountStateEvent reset_account_event(self)
    cpdef datetime time_now(self)
    cpdef void change_fill_model(self, FillModel fill_model) except *
    cpdef void process_tick(self, QuoteTick tick) except *
    cpdef void adjust_account(self, OrderFillEvent event, Position position) except *
    cpdef Money calculate_pnl(self, MarketPosition direction, double open_price, double close_price, Quantity quantity, double exchange_rate)
    cpdef void apply_rollover_interest(self, datetime timestamp, int iso_week_day) except *

    cpdef void handle_account_inquiry(self, AccountInquiry command) except *
    cpdef void handle_submit_order(self, SubmitOrder command) except *
    cpdef void handle_submit_bracket_order(self, SubmitBracketOrder command) except *
    cpdef void handle_modify_order(self, ModifyOrder command) except *
    cpdef void handle_cancel_order(self, CancelOrder command) except *

    # -- EVENT HANDLING --------------------------------------------------------------------------------
    cdef bint _is_marginal_buy_stop_fill(self, Price order_price, QuoteTick current_market)
    cdef bint _is_marginal_buy_limit_fill(self, Price order_price, QuoteTick current_market)
    cdef bint _is_marginal_sell_stop_fill(self, Price order_price, QuoteTick current_market)
    cdef bint _is_marginal_sell_limit_fill(self, Price order_price, QuoteTick current_market)
    cdef void _submit_order(self, Order order) except *
    cdef void _accept_order(self, Order order) except *
    cdef void _reject_order(self, Order order, str reason) except *
    cdef void _cancel_reject_order(self, OrderId order_id, str response, str reason) except *
    cdef void _expire_order(self, PassiveOrder order) except *
    cdef void _process_order(self, Order order) except *
    cdef void _process_market_order(self, MarketOrder order, QuoteTick current_market) except *
    cdef void _process_limit_order(self, LimitOrder order, QuoteTick current_market) except *
    cdef void _process_passive_order(self, PassiveOrder order, QuoteTick current_market) except *
    cdef void _work_order(self, Order order) except *
    cdef void _fill_order(self, Order order, Price fill_price, LiquiditySide liquidity_side) except *
    cdef void _clean_up_child_orders(self, OrderId order_id) except *
    cdef void _check_oco_order(self, OrderId order_id) except *
    cdef void _reject_oco_order(self, PassiveOrder order, OrderId oco_order_id) except *
    cdef void _cancel_oco_order(self, PassiveOrder order, OrderId oco_order_id) except *
    cdef void _cancel_order(self, PassiveOrder order) except *
