# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from libc.stdint cimport uint64_t

from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef class FillModel:
    cdef readonly double prob_fill_on_limit
    """The probability of limit orders filling on the limit price.\n\n:returns: `bool`"""
    cdef readonly double prob_fill_on_stop
    """The probability of stop orders filling on the stop price.\n\n:returns: `bool`"""
    cdef readonly double prob_slippage
    """The probability of aggressive order execution slipping.\n\n:returns: `bool`"""

    cpdef bint is_limit_filled(self)
    cpdef bint is_stop_filled(self)
    cpdef bint is_slipped(self)

    cdef bint _event_success(self, double probability)


cdef class LatencyModel:
    cdef readonly uint64_t base_latency_nanos
    """The default latency to the exchange.\n\n:returns: `int`"""
    cdef readonly uint64_t insert_latency_nanos
    """The latency (nanoseconds) for order insert messages to reach the exchange.\n\n:returns: `int`"""
    cdef readonly uint64_t update_latency_nanos
    """The latency (nanoseconds) for order update messages to reach the exchange.\n\n:returns: `int`"""
    cdef readonly uint64_t cancel_latency_nanos
    """The latency (nanoseconds) for order cancel messages to reach the exchange.\n\n:returns: `int`"""


cdef class FeeModel:
    cpdef Money get_commission(self, Order order, Quantity fill_qty, Price fill_px, Instrument instrument)


cdef class MakerTakerFeeModel(FeeModel):
    pass


cdef class FixedFeeModel(FeeModel):
    cdef Money _commission
    cdef Money _zero_commission
    cdef bint _charge_commission_once


cdef class PerContractFeeModel(FeeModel):
    cdef Money _commission
