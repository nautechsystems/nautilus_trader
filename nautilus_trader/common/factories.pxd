# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/gpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
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
