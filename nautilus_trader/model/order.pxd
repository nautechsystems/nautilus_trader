# -------------------------------------------------------------------------------------------------
# <copyright file="order.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.core.decimal cimport Decimal
from nautilus_trader.core.types cimport GUID, Label
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.order_purpose cimport OrderPurpose
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.objects cimport Quantity, Price
from nautilus_trader.model.events cimport OrderEvent, OrderInitialized
from nautilus_trader.model.identifiers cimport Symbol, OrderId, OrderIdBroker
from nautilus_trader.model.identifiers cimport AtomicOrderId, AccountId, ExecutionId
from nautilus_trader.model.identifiers cimport PositionIdBroker


cdef class Order:
    cdef set _execution_ids
    cdef list _events

    cdef readonly OrderId id
    cdef readonly OrderIdBroker id_broker
    cdef readonly AccountId account_id
    cdef readonly ExecutionId execution_id
    cdef readonly PositionIdBroker position_id_broker
    cdef readonly Symbol symbol
    cdef readonly OrderSide side
    cdef readonly OrderType type
    cdef readonly OrderState state
    cdef readonly Quantity quantity
    cdef readonly datetime timestamp
    cdef readonly Price price
    cdef readonly Label label
    cdef readonly OrderPurpose purpose
    cdef readonly TimeInForce time_in_force
    cdef readonly datetime expire_time
    cdef readonly Quantity filled_quantity
    cdef readonly datetime filled_timestamp
    cdef readonly Price average_price
    cdef readonly Decimal slippage
    cdef readonly GUID init_id
    cdef readonly OrderEvent last_event
    cdef readonly int event_count
    cdef readonly bint is_buy
    cdef readonly bint is_sell
    cdef readonly bint is_working
    cdef readonly bint is_completed

    @staticmethod
    cdef Order create(OrderInitialized event)
    cpdef bint equals(self, Order other)
    cpdef str status_string(self)
    cpdef str state_as_string(self)
    cpdef list get_execution_ids(self)
    cpdef list get_events(self)
    cpdef void apply(self, OrderEvent event) except *
    cdef void _set_is_working_true(self) except *
    cdef void _set_is_completed_true(self) except *
    cdef void _set_filled_state(self) except *
    cdef void _set_slippage(self) except *


cdef class AtomicOrder:
    cdef readonly AtomicOrderId id
    cdef readonly Order entry
    cdef readonly Order stop_loss
    cdef readonly Order take_profit
    cdef readonly bint has_take_profit
    cdef readonly datetime timestamp

    cpdef bint equals(self, AtomicOrder other)
