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

from typing import Callable

from cpython.datetime cimport timedelta
from libc.stdint cimport int64_t
from libc.stdint cimport uint64_t

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.clock cimport TimeEvent
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.fsm cimport FiniteStateMachine
from nautilus_trader.core.message cimport Request
from nautilus_trader.core.message cimport Response
from nautilus_trader.core.rust.common cimport ComponentState
from nautilus_trader.core.rust.common cimport ComponentTrigger
from nautilus_trader.core.rust.common cimport MessageBus_API
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.identifiers cimport Identifier
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.serialization.base cimport Serializer


cpdef ComponentState component_state_from_str(str value)
cpdef str component_state_to_str(ComponentState value)

cpdef ComponentTrigger component_trigger_from_str(str value)
cpdef str component_trigger_to_str(ComponentTrigger value)


cdef class ComponentFSMFactory:

    @staticmethod
    cdef create()


cdef class Component:
    cdef readonly Clock _clock
    cdef readonly LoggerAdapter _log
    cdef readonly MessageBus _msgbus
    cdef FiniteStateMachine _fsm
    cdef dict _config

    cdef readonly TraderId trader_id
    """The trader ID associated with the component.\n\n:returns: `TraderId`"""
    cdef readonly Identifier id
    """The components ID.\n\n:returns: `ComponentId`"""
    cdef readonly type type
    """The components type.\n\n:returns: `type`"""

    cdef void _change_clock(self, Clock clock)
    cdef void _change_logger(self, Logger logger)
    cdef void _change_msgbus(self, MessageBus msgbus)

# -- ABSTRACT METHODS -----------------------------------------------------------------------------

    cpdef void _start(self)
    cpdef void _stop(self)
    cpdef void _resume(self)
    cpdef void _reset(self)
    cpdef void _dispose(self)
    cpdef void _degrade(self)
    cpdef void _fault(self)

# -- COMMANDS -------------------------------------------------------------------------------------

    cdef void _initialize(self)
    cpdef void start(self)
    cpdef void stop(self)
    cpdef void resume(self)
    cpdef void reset(self)
    cpdef void dispose(self)
    cpdef void degrade(self)
    cpdef void fault(self)

# --------------------------------------------------------------------------------------------------

    cdef void _trigger_fsm(
        self,
        ComponentTrigger trigger,
        bint is_transitory,
        action: Callable[[None], None]=*,
    )


cdef class MessageBus:
    cdef MessageBus_API _mem
    cdef Clock _clock
    cdef LoggerAdapter _log
    cdef Serializer _serializer
    cdef dict[Subscription, list[str]] _subscriptions
    cdef dict[str, Subscription[:]] _patterns
    cdef dict[str, object] _endpoints
    cdef dict[UUID4, object] _correlation_index
    cdef bint _has_backing
    cdef tuple[type] _publishable_types

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
    cpdef bint is_subscribed(self, str topic, handler)
    cpdef bint is_pending_request(self, UUID4 request_id)

    cpdef void register(self, str endpoint, handler)
    cpdef void deregister(self, str endpoint, handler)
    cpdef void send(self, str endpoint, msg)
    cpdef void request(self, str endpoint, Request request)
    cpdef void response(self, Response response)
    cpdef void subscribe(self, str topic, handler, int priority=*)
    cpdef void unsubscribe(self, str topic, handler)
    cpdef void publish(self, str topic, msg)
    cdef void publish_c(self, str topic, msg)
    cdef Subscription[:] _resolve_subscriptions(self, str topic)


cdef bint is_matching(str topic, str pattern)


cdef class Subscription:
    cdef readonly str topic
    """The topic for the subscription.\n\n:returns: `str`"""
    cdef readonly object handler
    """The handler for the subscription.\n\n:returns: `Callable`"""
    cdef readonly int priority
    """The priority for the subscription.\n\n:returns: `int`"""


cdef class Throttler:
    cdef Clock _clock
    cdef LoggerAdapter _log
    cdef uint64_t _interval_ns
    cdef object _buffer
    cdef str _timer_name
    cdef object _timestamps
    cdef object _output_send
    cdef object _output_drop
    cdef bint _warm

    cdef readonly str name
    """The name of the throttler.\n\n:returns: `str`"""
    cdef readonly int limit
    """The limit for the throttler rate.\n\n:returns: `int`"""
    cdef readonly timedelta interval
    """The interval for the throttler rate.\n\n:returns: `timedelta`"""
    cdef readonly bint is_limiting
    """If the throttler is currently limiting messages (buffering or dropping).\n\n:returns: `bool`"""
    cdef readonly int recv_count
    """If count of messages received by the throttler.\n\n:returns: `int`"""
    cdef readonly int sent_count
    """If count of messages sent from the throttler.\n\n:returns: `int`"""

    cpdef double used(self)
    cpdef void send(self, msg)
    cdef int64_t _delta_next(self)
    cdef void _limit_msg(self, msg)
    cdef void _set_timer(self, handler: Callable[[TimeEvent], None])
    cpdef void _process(self, TimeEvent event)
    cpdef void _resume(self, TimeEvent event)
    cdef void _send_msg(self, msg)
