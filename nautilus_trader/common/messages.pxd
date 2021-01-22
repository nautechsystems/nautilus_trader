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

from nautilus_trader.core.message cimport Request
from nautilus_trader.core.message cimport Response
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.commands cimport TradingCommand


cdef class Subscribe(TradingCommand):
    cdef readonly type data_type
    """The subscription data type.\n\n:returns: `type`"""
    cdef readonly dict metadata
    """The subscription metadata.\n\n:returns: `dict`"""
    cdef readonly object handler
    """The handler for the subscription.\n\n:returns: `callable`"""


cdef class Unsubscribe(TradingCommand):
    cdef readonly type data_type
    """The subscription data type.\n\n:returns: `type`"""
    cdef readonly dict metadata
    """The subscription metadata.\n\n:returns: `dict`"""
    cdef readonly object handler
    """The handler for the subscription.\n\n:returns: `callable`"""


cdef class DataRequest(Request):
    cdef readonly Venue venue
    """The venue for the request.\n\n:returns: `Venue`"""
    cdef readonly type data_type
    """The request data type.\n\n:returns: `type`"""
    cdef readonly dict metadata
    """The request metadata.\n\n:returns: `dict`"""
    cdef readonly object callback
    """The callback to receive the data.\n\n:returns: `callable`"""


cdef class DataResponse(Response):
    cdef readonly Venue venue
    """The venue of the response.\n\n:returns: `Venue`"""
    cdef readonly type data_type
    """The response data type.\n\n:returns: `type`"""
    cdef readonly dict metadata
    """The response metadata.\n\n:returns: `dict`"""
    cdef readonly list data
    """The response data.\n\n:returns: `list`"""
