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

from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport Venue


cdef class ExecutionStateReport:
    cdef readonly Venue venue
    """The venue for the report.\n\n:returns: `Venue`"""
    cdef readonly AccountId account_id
    """The account identifier for the venue.\n\n:returns: `AccountId`"""
    cdef readonly dict order_states
    """The order states for the venue.\n\n:returns: `dict[OrderId, OrderState]`"""
    cdef readonly dict order_filled
    """The order fill info for the venue.\n\n:returns: `dict[OrderId, OrderEvent]`"""
    cdef readonly dict position_states
    """The position states for the venue.\n\n:returns: `dict[Security, Decimal]`"""
