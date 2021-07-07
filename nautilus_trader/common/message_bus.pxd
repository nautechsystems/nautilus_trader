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

from typing import Callable

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.type cimport MessageType


cdef class Subscription:
    cdef readonly MessageType msg_type
    """The message type for the subscription.\n\n:returns: `MessageType`"""
    cdef readonly object handler
    """The handler for the subscription.\n\n:returns: `Callable`"""
    cdef readonly int priority
    """The priority for the subscription.\n\n:returns: `int`"""


cdef class MessageBus:
    cdef Clock _clock
    cdef UUIDFactory _uuid_factory
    cdef LoggerAdapter _log
    cdef dict _channels

    cdef readonly int processed_count
    """The count of messages process by the bus.\n\n:returns: `int32`"""

    cpdef list channels(self)
    cpdef list subscriptions(self, MessageType msg_type)

    cpdef void subscribe(self, MessageType msg_type, handler: Callable, int priority=*) except *
    cpdef void unsubscribe(self, MessageType msg_type, handler: Callable) except *
    cpdef void publish(self, MessageType msg_type, message) except *
