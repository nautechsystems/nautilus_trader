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

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LoggerAdapter


cdef class Subscription:
    cdef str _topic_str

    cdef readonly str topic
    """The topic for the subscription.\n\n:returns: `str`"""
    cdef readonly object handler
    """The handler for the subscription.\n\n:returns: `Callable`"""
    cdef readonly int priority
    """The priority for the subscription.\n\n:returns: `int`"""


cdef class MessageBus:
    cdef Clock _clock
    cdef LoggerAdapter _log
    cdef dict _channels
    cdef Subscription[:] _patterns
    cdef int _patterns_len

    cdef readonly int processed_count
    """The count of messages process by the bus.\n\n:returns: `int32`"""

    cpdef list channels(self)
    cpdef list subscriptions(self, str topic)

    cpdef void subscribe(self, str topic, handler, int priority=*) except *
    cdef void _subscribe_pattern(self, Subscription sub) except *
    cdef void _subscribe_channel(self, Subscription sub) except *
    cpdef void unsubscribe(self, str topic, handler) except *
    cdef void _unsubscribe_pattern(self, Subscription sub) except *
    cdef void _unsubscribe_channel(self, Subscription sub) except *
    cpdef void publish(self, str topic, msg) except *
    cdef void publish_c(self, str topic, msg) except *
