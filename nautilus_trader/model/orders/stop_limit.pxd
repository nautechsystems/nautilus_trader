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

from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport PassiveOrder


cdef class StopLimitOrder(PassiveOrder):
    cdef readonly Price trigger
    """The trigger stop price for the order.\n\n:returns: `Price`"""
    cdef readonly bint is_triggered
    """If the order has been triggered.\n\n:returns: `bool`"""
    cdef readonly bint is_post_only
    """If the order will only make liquidity.\n\n:returns: `bool`"""
    cdef readonly bint is_reduce_only
    """If the order will only reduce an open position.\n\n:returns: `bool`"""
    cdef readonly bint is_hidden
    """If the order is hidden from the public book.\n\n:returns: `bool`"""

    @staticmethod
    cdef StopLimitOrder create(OrderInitialized init)
