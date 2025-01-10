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

from typing import Callable

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta
from cpython.datetime cimport tzinfo
from libc.stdint cimport int64_t
from libc.stdint cimport uint64_t

from nautilus_trader.core.fsm cimport FiniteStateMachine
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.message cimport Request
from nautilus_trader.core.message cimport Response
from nautilus_trader.core.rust.common cimport ComponentState
from nautilus_trader.core.rust.common cimport ComponentTrigger
from nautilus_trader.core.rust.common cimport LiveClock_API
from nautilus_trader.core.rust.common cimport LogColor
from nautilus_trader.core.rust.common cimport LogGuard_API
from nautilus_trader.core.rust.common cimport LogLevel
from nautilus_trader.core.rust.common cimport TestClock_API
from nautilus_trader.core.rust.common cimport TimeEvent_t
from nautilus_trader.core.rust.core cimport CVec
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.identifiers cimport Identifier
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.serialization.base cimport Serializer


cdef class Clock:
    cpdef double timestamp(self)
    cpdef uint64_t timestamp_ms(self)
    cpdef uint64_t timestamp_us(self)
    cpdef uint64_t timestamp_ns(self)
    cpdef datetime utc_now(self)
    cpdef datetime local_now(self, tzinfo tz=*)
    cpdef uint64_t next_time_ns(self, str name)
    cpdef void register_default_handler(self, handler: Callable[[TimeEvent], None])
    cpdef void set_time_alert(
        self,
        str name,
        datetime alert_time,
        callback: Callable[[TimeEvent], None]=*,
        bint override=*,
    )
    cpdef void set_time_alert_ns(
        self,
        str name,
        uint64_t alert_time_ns,
        callback: Callable[[TimeEvent], None]=*,
    )
    cpdef void set_timer(
        self,
        str name,
        timedelta interval,
        datetime start_time=*,
        datetime stop_time=*,
        callback: Callable[[TimeEvent], None]=*,
    )
    cpdef void set_timer_ns(
        self,
        str name,
        uint64_t interval_ns,
        uint64_t start_time_ns,
        uint64_t stop_time_ns,
        callback: Callable[[TimeEvent], None]=*,
    )
    cpdef void cancel_timer(self, str name)
    cpdef void cancel_timers(self)


cdef dict[UUID4, Clock] _COMPONENT_CLOCKS

cdef list[TestClock] get_component_clocks(UUID4 instance_id)
cpdef void register_component_clock(UUID4 instance_id, Clock clock)
cpdef void deregister_component_clock(UUID4 instance_id, Clock clock)


cdef bint FORCE_STOP

cpdef void set_backtest_force_stop(bint value)
cpdef bint is_backtest_force_stop()


cdef class TestClock(Clock):
    cdef TestClock_API _mem

    cpdef void set_time(self, uint64_t to_time_ns)
    cdef CVec advance_time_c(self, uint64_t to_time_ns, bint set_time=*)
    cpdef list advance_time(self, uint64_t to_time_ns, bint set_time=*)


cdef class LiveClock(Clock):
    cdef LiveClock_API _mem


cdef class TimeEvent(Event):
    cdef TimeEvent_t _mem

    cdef str to_str(self)

    @staticmethod
    cdef TimeEvent from_mem_c(TimeEvent_t raw)


cdef class TimeEventHandler:
    cdef object _handler
    cdef readonly TimeEvent event
    """The handlers event.\n\n:returns: `TimeEvent`"""

    cpdef void handle(self)


cdef str RECV
cdef str SENT
cdef str CMD
cdef str EVT
cdef str DOC
cdef str RPT
cdef str REQ
cdef str RES


cdef void set_logging_clock_realtime_mode()
cdef void set_logging_clock_static_mode()
cdef void set_logging_clock_static_time(uint64_t time_ns)

cpdef LogColor log_color_from_str(str value)
cpdef str log_color_to_str(LogColor value)

cpdef LogLevel log_level_from_str(str value)
cpdef str log_level_to_str(LogLevel value)


cdef class LogGuard:
    cdef LogGuard_API _mem


cpdef LogGuard init_logging(
    TraderId trader_id=*,
    str machine_id=*,
    UUID4 instance_id=*,
    LogLevel level_stdout=*,
    LogLevel level_file=*,
    str directory=*,
    str file_name=*,
    str file_format=*,
    dict component_levels=*,
    bint colors=*,
    bint bypass=*,
    bint print_config=*,
)

# Global static to flag if pyo3 based logging is initialized
cdef bint LOGGING_PYO3
cpdef bint is_logging_initialized()
cpdef void set_logging_pyo3(bint value)


cdef class Logger:
    cdef str _name
    cdef const char* _name_ptr

    cpdef void debug(self, str message, LogColor color=*)
    cpdef void info(self, str message, LogColor color=*)
    cpdef void warning(self, str message, LogColor color=*)
    cpdef void error(self, str message, LogColor color=*)
    cpdef void exception(self, str message, ex)


cpdef void log_header(
    TraderId trader_id,
    str machine_id,
    UUID4 instance_id,
    str component,
)


cpdef void log_sysinfo(str component)


cpdef ComponentState component_state_from_str(str value)
cpdef str component_state_to_str(ComponentState value)

cpdef ComponentTrigger component_trigger_from_str(str value)
cpdef str component_trigger_to_str(ComponentTrigger value)


cdef class ComponentFSMFactory:

    @staticmethod
    cdef create()


cdef class Component:
    cdef readonly Clock _clock
    cdef readonly Logger _log
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
    cpdef void shutdown_system(self, str reason=*)

# --------------------------------------------------------------------------------------------------

    cdef void _trigger_fsm(
        self,
        ComponentTrigger trigger,
        bint is_transitory,
        action: Callable[[None], None]=*,
    )


cdef class MessageBus:
    cdef Clock _clock
    cdef Logger _log
    cdef object _database
    cdef dict[Subscription, list[str]] _subscriptions
    cdef dict[str, Subscription[:]] _patterns
    cdef dict[str, object] _endpoints
    cdef dict[UUID4, object] _correlation_index
    cdef tuple[type] _publishable_types
    cdef set[type] _streaming_types
    cdef bint _resolved

    cdef readonly TraderId trader_id
    """The trader ID associated with the bus.\n\n:returns: `TraderId`"""
    cdef readonly Serializer serializer
    """The serializer for the bus.\n\n:returns: `Serializer`"""
    cdef readonly bint has_backing
    """If the message bus has a database backing.\n\n:returns: `bool`"""
    cdef readonly uint64_t sent_count
    """The count of messages sent through the bus.\n\n:returns: `uint64_t`"""
    cdef readonly uint64_t req_count
    """The count of requests processed by the bus.\n\n:returns: `uint64_t`"""
    cdef readonly uint64_t res_count
    """The count of responses processed by the bus.\n\n:returns: `uint64_t`"""
    cdef readonly uint64_t pub_count
    """The count of messages published by the bus.\n\n:returns: `uint64_t`"""

    cpdef list endpoints(self)
    cpdef list topics(self)
    cpdef list subscriptions(self, str pattern=*)
    cpdef bint has_subscribers(self, str pattern=*)
    cpdef bint is_subscribed(self, str topic, handler)
    cpdef bint is_pending_request(self, UUID4 request_id)
    cpdef bint is_streaming_type(self, type cls)

    cpdef void dispose(self)
    cpdef void register(self, str endpoint, handler)
    cpdef void deregister(self, str endpoint, handler)
    cpdef void add_streaming_type(self, type cls)
    cpdef void send(self, str endpoint, msg)
    cpdef void request(self, str endpoint, Request request)
    cpdef void response(self, Response response)
    cpdef void subscribe(self, str topic, handler, int priority=*)
    cpdef void unsubscribe(self, str topic, handler)
    cpdef void publish(self, str topic, msg, bint external_pub=*)
    cdef void publish_c(self, str topic, msg, bint external_pub=*)
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
    cdef Logger _log
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

    cpdef void reset(self)
    cpdef double used(self)
    cpdef void send(self, msg)
    cdef int64_t _delta_next(self)
    cdef void _limit_msg(self, msg)
    cdef void _set_timer(self, handler: Callable[[TimeEvent], None])
    cpdef void _process(self, TimeEvent event)
    cpdef void _resume(self, TimeEvent event)
    cdef void _send_msg(self, msg)
