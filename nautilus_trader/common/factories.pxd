# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.core.types cimport Label
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_purpose cimport OrderPurpose
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.objects cimport Quantity, Price
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.generators cimport OrderIdGenerator
from nautilus_trader.model.order cimport Order, AtomicOrder
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory


cdef class OrderFactory:
    cdef Clock _clock
    cdef GuidFactory _guid_factory
    cdef OrderIdGenerator _id_generator

    cpdef int count(self)
    cpdef void set_count(self, int count) except *
    cpdef void reset(self) except *

    cpdef Order market(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Label label=*,
            OrderPurpose order_purpose=*)

    cpdef Order limit(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price price,
            Label label=*,
            OrderPurpose order_purpose=*,
            TimeInForce time_in_force=*,
            datetime expire_time=*)

    cpdef Order stop(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price price,
            Label label=*,
            OrderPurpose order_purpose=*,
            TimeInForce time_in_force=*,
            datetime expire_time=*)

    cpdef Order stop_limit(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price price,
            Label label=*,
            OrderPurpose order_purpose=*,
            TimeInForce time_in_force=*,
            datetime expire_time=*)

    cpdef Order market_if_touched(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price price,
            Label label=*,
            OrderPurpose order_purpose=*,
            TimeInForce time_in_force=*,
            datetime expire_time=*)

    cpdef Order fill_or_kill(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Label label=*,
            OrderPurpose order_purpose=*,)

    cpdef Order immediate_or_cancel(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Label label=*,
            OrderPurpose order_purpose=*)

    cpdef AtomicOrder atomic_market(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price stop_loss,
            Price take_profit=*,
            Label label=*)

    cpdef AtomicOrder atomic_limit(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price entry,
            Price stop_loss,
            Price take_profit=*,
            Label label=*,
            TimeInForce time_in_force=*,
            datetime expire_time=*)

    cpdef AtomicOrder atomic_stop_market(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price entry,
            Price stop_loss,
            Price take_profit=*,
            Label label=*,
            TimeInForce time_in_force=*,
            datetime expire_time=*)

    cdef AtomicOrder _create_atomic_order(
        self,
        Order entry_order,
        Price stop_loss,
        Price take_profit,
        Label original_label)
