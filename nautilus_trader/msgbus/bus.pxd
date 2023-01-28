# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.message cimport Request
from nautilus_trader.core.message cimport Response
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.msgbus.subscription cimport Subscription


cdef class MessageBus:
    cdef Clock _clock
    cdef LoggerAdapter _log
    cdef dict _subscriptions
    cdef dict _patterns
    cdef dict _endpoints
    cdef dict _correlation_index

    cdef readonly TraderId trader_id
    """The trader ID associated with the bus.\n\n:returns: `TraderId`"""
    cdef readonly int sent_count
    """The count of messages sent through the bus.\n\n:returns: `int`"""
    cdef readonly int req_count
    """The count of requests processed by the bus.\n\n:returns: `int`"""
    cdef readonly int res_count
    """The count of responses processed by the bus.\n\n:returns: `int`"""
    cdef readonly int pub_count
    """The count of messages published by the bus.\n\n:returns: `int`"""

    cpdef list endpoints(self)
    cpdef list topics(self)
    cpdef list subscriptions(self, str pattern=*)
    cpdef bint has_subscribers(self, str pattern=*)
    cpdef bint is_subscribed(self, str topic, handler) except *

    cpdef void register(self, str endpoint, handler) except *
    cpdef void deregister(self, str endpoint, handler) except *
    cpdef void send(self, str endpoint, msg) except *
    cpdef void request(self, str endpoint, Request request) except *
    cpdef void response(self, Response response) except *
    cpdef void subscribe(self, str topic, handler, int priority=*) except *
    cpdef void unsubscribe(self, str topic, handler) except *
    cpdef void publish(self, str topic, msg) except *
    cdef void publish_c(self, str topic, msg) except *
    cdef Subscription[:] _resolve_subscriptions(self, str topic)


cdef bint is_matching(str topic, str pattern) except *
