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

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.generators cimport OrderIdGenerator
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.order cimport BracketOrder
from nautilus_trader.model.order cimport MarketOrder
from nautilus_trader.model.order cimport LimitOrder
from nautilus_trader.model.order cimport StopOrder


cdef class OrderFactory:
    cdef Clock _clock
    cdef UUIDFactory _uuid_factory
    cdef OrderIdGenerator _id_generator

    cpdef int count(self)
    cpdef void set_count(self, int count) except *
    cpdef void reset(self) except *

    cpdef MarketOrder market(
        self,
        Symbol symbol,
        OrderSide order_side,
        Quantity quantity,
        TimeInForce time_in_force=*)

    cpdef LimitOrder limit(
        self,
        Symbol symbol,
        OrderSide order_side,
        Quantity quantity,
        Price price,
        TimeInForce time_in_force=*,
        datetime expire_time=*,
        bint is_post_only=*,
        bint is_hidden=*)

    cpdef StopOrder stop(
        self,
        Symbol symbol,
        OrderSide order_side,
        Quantity quantity,
        Price price,
        TimeInForce time_in_force=*,
        datetime expire_time=*)

    cpdef BracketOrder bracket(
        self,
        Order entry_order,
        Price stop_loss,
        Price take_profit=*)
