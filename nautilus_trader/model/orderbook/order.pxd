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

from nautilus_trader.model.c_enums.order_side cimport OrderSide


cdef class Order:
    cdef readonly double price
    """The orders price.\n\n:returns: `double`"""
    cdef readonly double size
    """The orders size.\n\n:returns: `double`"""
    cdef readonly OrderSide side
    """The orders side.\n\n:returns: `OrderSide`"""
    cdef readonly str id
    """The orders ID.\n\n:returns: `str`"""

    cpdef void update_price(self, double price) except *
    cpdef void update_size(self, double size) except *
    cpdef void update_id(self, str value) except *
    cpdef double exposure(self)
    cpdef double signed_size(self)

    @staticmethod
    cdef Order from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(Order obj)
