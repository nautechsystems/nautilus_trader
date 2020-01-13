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
    cdef readonly double rollover_spread
    cdef readonly Money total_commissions
    cdef readonly Money total_rollover
    cdef readonly FillModel fill_model

    cdef dict _market
    cdef dict _slippages
    cdef dict _min_stops
    cdef dict _min_limits

    cdef dict _working_orders
    cdef dict _atomic_child_orders
    cdef dict _oco_orders

    cdef void _set_slippages(self) except *
    cdef void _set_min_distances(self) except *
    cpdef datetime time_now(self)
    cpdef void register_exec_db(self, ExecutionDatabase exec_db) except *
    cpdef void change_fill_model(self, FillModel fill_model) except *
    cpdef void process_tick(self, Tick tick) except *
    cpdef void check_residuals(self) except *
    cpdef void reset(self) except *

    cdef AccountStateEvent reset_account_event(self)

# -- EVENT HANDLING ------------------------------------------------------------------------------ #
    cdef void _accept_order(self, Order order) except *
    cdef void _reject_order(self, Order order, str reason) except *
    cdef void _cancel_reject_order(self, OrderId order_id, str response, str reason) except *
    cdef void _expire_order(self, Order order) except *
    cdef void _process_order(self, Order order) except *
    cdef void _fill_order(self, Order order, Price fill_price) except *
    cdef void _clean_up_child_orders(self, OrderId order_id) except *
    cdef void _check_oco_order(self, OrderId order_id) except *
    cdef void _reject_oco_order(self, Order order, OrderId oco_order_id) except *
    cdef void _cancel_oco_order(self, Order order, OrderId oco_order_id) except *
    cdef void _adjust_account(self, OrderFillEvent event, Position position) except *
    cdef void _apply_rollover_interest(self, datetime timestamp, int iso_week_day) except *
    cdef dict _build_current_bid_rates(self)
    cdef dict _build_current_ask_rates(self)
    cdef Money _calculate_pnl(self, MarketPosition direction, double open_price, double close_price, Quantity quantity, double exchange_rate)
