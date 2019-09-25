# -------------------------------------------------------------------------------------------------
# <copyright file="commands.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.types cimport ValidString
from nautilus_trader.core.message cimport Command
from nautilus_trader.model.identifiers cimport OrderId, TraderId, StrategyId, PositionId, AccountId
from nautilus_trader.model.objects cimport Price, Quantity
from nautilus_trader.model.order cimport Order, AtomicOrder


cdef class AccountInquiry(Command):
    cdef readonly TraderId trader_id
    cdef readonly AccountId account_id


cdef class SubmitOrder(Command):
    cdef readonly TraderId trader_id
    cdef readonly AccountId account_id
    cdef readonly StrategyId strategy_id
    cdef readonly PositionId position_id
    cdef readonly Order order


cdef class SubmitAtomicOrder(Command):
    cdef readonly TraderId trader_id
    cdef readonly AccountId account_id
    cdef readonly StrategyId strategy_id
    cdef readonly PositionId position_id
    cdef readonly AtomicOrder atomic_order


cdef class ModifyOrder(Command):
    cdef readonly TraderId trader_id
    cdef readonly AccountId account_id
    cdef readonly OrderId order_id
    cdef readonly Quantity modified_quantity
    cdef readonly Price modified_price


cdef class CancelOrder(Command):
    cdef readonly TraderId trader_id
    cdef readonly AccountId account_id
    cdef readonly OrderId order_id
    cdef readonly ValidString cancel_reason
