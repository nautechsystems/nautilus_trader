# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.common.account cimport Account
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.brokerage cimport CommissionCalculator, RolloverInterestCalculator
from nautilus_trader.common.execution cimport ExecutionDatabase, ExecutionClient
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.market_position cimport MarketPosition
from nautilus_trader.model.events cimport AccountStateEvent, OrderFillEvent
from nautilus_trader.model.currency cimport ExchangeRateCalculator
from nautilus_trader.model.objects cimport Price, Tick, Bar, Money, Quantity
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.identifiers cimport Symbol, OrderId
from nautilus_trader.backtest.models cimport FillModel


cdef class BacktestExecClient(ExecutionClient):
    cdef readonly Clock _clock
    cdef readonly GuidFactory _guid_factory
    cdef readonly Account _account
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
    cdef readonly ExecutionDatabase exec_db
    cdef readonly ExchangeRateCalculator exchange_calculator
    cdef readonly CommissionCalculator commission_calculator
    cdef readonly RolloverInterestCalculator rollover_calculator
    cdef readonly object rollover_spread
    cdef readonly Money total_commissions
    cdef readonly Money total_rollover
    cdef readonly FillModel fill_model
    cdef readonly dict current_bids
    cdef readonly dict current_asks
    cdef readonly dict slippage_index
    cdef readonly dict working_orders
    cdef readonly dict atomic_child_orders
    cdef readonly dict oco_orders

    cpdef datetime time_now(self)
    cpdef void register_exec_db(self, ExecutionDatabase exec_db)
    cpdef void change_fill_model(self, FillModel fill_model)
    cpdef void process_tick(self, Tick tick)
    #cpdef void process_bars(self, Symbol symbol, Bar bid_bar, Bar ask_bar)
    cdef void _process_market(self, Symbol symbol, Price lowest_bid, Price highest_ask)
    cpdef void check_residuals(self)
    cpdef void reset(self)

    cdef AccountStateEvent reset_account_event(self)
    cdef void _set_slippage_index(self)

# -- EVENT HANDLING ------------------------------------------------------------------------------ #
    cdef void _accept_order(self, Order order)
    cdef void _reject_order(self, Order order, str reason)
    cdef void _cancel_reject_order(self, OrderId order_id, str response, str reason)
    cdef void _expire_order(self, Order order)
    cdef void _process_order(self, Order order)
    cdef void _fill_order(self, Order order, Price fill_price)
    cdef void _clean_up_child_orders(self, OrderId order_id)
    cdef void _check_oco_order(self, OrderId order_id)
    cdef void _reject_oco_order(self, Order order, OrderId oco_order_id)
    cdef void _cancel_oco_order(self, Order order, OrderId oco_order_id)
    cdef void _adjust_account(self, OrderFillEvent event, Position position)
    cdef void _apply_rollover_interest(self, datetime timestamp, int iso_week_day)
    cdef dict _build_current_bid_rates(self)
    cdef dict _build_current_ask_rates(self)
    cdef Money _calculate_pnl(self, MarketPosition direction, open_price, close_price, Quantity quantity, float exchange_rate)
