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

from collections import deque
from typing import Any
from typing import Callable
from typing import Optional

from nautilus_trader.core.rust.common import ComponentState as PyComponentState

from cpython.datetime cimport timedelta
from libc.stdint cimport int64_t
from libc.stdint cimport uint64_t

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.clock cimport TimeEvent
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.messages cimport ComponentStateChanged
from nautilus_trader.common.msgbus cimport MessageBus
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.fsm cimport FiniteStateMachine
from nautilus_trader.core.fsm cimport InvalidStateTrigger
from nautilus_trader.core.rust.common cimport ComponentState
from nautilus_trader.core.rust.common cimport ComponentTrigger
from nautilus_trader.core.rust.common cimport component_state_from_cstr
from nautilus_trader.core.rust.common cimport component_state_to_cstr
from nautilus_trader.core.rust.common cimport component_trigger_from_cstr
from nautilus_trader.core.rust.common cimport component_trigger_to_cstr
from nautilus_trader.core.rust.core cimport secs_to_nanos
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.core.string cimport pystr_to_cstr
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.identifiers cimport ComponentId
from nautilus_trader.model.identifiers cimport Identifier
from nautilus_trader.model.identifiers cimport TraderId


cpdef ComponentState component_state_from_str(str value):
    return component_state_from_cstr(pystr_to_cstr(value))


cpdef str component_state_to_str(ComponentState value):
    return cstr_to_pystr(component_state_to_cstr(value))


cpdef ComponentTrigger component_trigger_from_str(str value):
    return component_trigger_from_cstr(pystr_to_cstr(value))


cpdef str component_trigger_to_str(ComponentTrigger value):
    return cstr_to_pystr(component_trigger_to_cstr(value))


cdef dict _COMPONENT_STATE_TABLE = {
    (ComponentState.PRE_INITIALIZED, ComponentTrigger.INITIALIZE): ComponentState.READY,
    (ComponentState.READY, ComponentTrigger.RESET): ComponentState.RESETTING,  # Transitional state
    (ComponentState.READY, ComponentTrigger.START): ComponentState.STARTING,  # Transitional state
    (ComponentState.READY, ComponentTrigger.DISPOSE): ComponentState.DISPOSING,  # Transitional state
    (ComponentState.RESETTING, ComponentTrigger.RESET_COMPLETED): ComponentState.READY,
    (ComponentState.STARTING, ComponentTrigger.START_COMPLETED): ComponentState.RUNNING,
    (ComponentState.STARTING, ComponentTrigger.STOP): ComponentState.STOPPING,  # Transitional state
    (ComponentState.STARTING, ComponentTrigger.FAULT): ComponentState.FAULTING,  # Transitional state
    (ComponentState.RUNNING, ComponentTrigger.STOP): ComponentState.STOPPING,  # Transitional state
    (ComponentState.RUNNING, ComponentTrigger.DEGRADE): ComponentState.DEGRADING,  # Transitional state
    (ComponentState.RUNNING, ComponentTrigger.FAULT): ComponentState.FAULTING,  # Transitional state
    (ComponentState.RESUMING, ComponentTrigger.STOP): ComponentState.STOPPING,  # Transitional state
    (ComponentState.RESUMING, ComponentTrigger.RESUME_COMPLETED): ComponentState.RUNNING,
    (ComponentState.RESUMING, ComponentTrigger.FAULT): ComponentState.FAULTING,  # Transitional state
    (ComponentState.STOPPING, ComponentTrigger.STOP_COMPLETED): ComponentState.STOPPED,
    (ComponentState.STOPPING, ComponentTrigger.FAULT): ComponentState.FAULTING,  # Transitional state
    (ComponentState.STOPPED, ComponentTrigger.RESET): ComponentState.RESETTING,  # Transitional state
    (ComponentState.STOPPED, ComponentTrigger.RESUME): ComponentState.RESUMING,  # Transitional state
    (ComponentState.STOPPED, ComponentTrigger.DISPOSE): ComponentState.DISPOSING,  # Transitional state
    (ComponentState.STOPPED, ComponentTrigger.FAULT): ComponentState.FAULTING,  # Transitional state
    (ComponentState.DEGRADING, ComponentTrigger.DEGRADE_COMPLETED): ComponentState.DEGRADED,
    (ComponentState.DEGRADED, ComponentTrigger.RESUME): ComponentState.RESUMING,  # Transitional state
    (ComponentState.DEGRADED, ComponentTrigger.STOP): ComponentState.STOPPING,  # Transitional state
    (ComponentState.DEGRADED, ComponentTrigger.FAULT): ComponentState.FAULTING,  # Transition state
    (ComponentState.DISPOSING, ComponentTrigger.DISPOSE_COMPLETED): ComponentState.DISPOSED,  # Terminal state
    (ComponentState.FAULTING, ComponentTrigger.FAULT_COMPLETED): ComponentState.FAULTED,  # Terminal state
}

cdef class ComponentFSMFactory:
    """
    Provides a generic component Finite-State Machine.
    """

    @staticmethod
    def get_state_transition_table() -> dict:
        """
        The default state transition table.

        Returns
        -------
        dict[int, int]
            C Enums.

        """
        return _COMPONENT_STATE_TABLE.copy()

    @staticmethod
    cdef create():
        """
        Create a new generic component FSM.

        Returns
        -------
        FiniteStateMachine

        """
        return FiniteStateMachine(
            state_transition_table=ComponentFSMFactory.get_state_transition_table(),
            initial_state=ComponentState.PRE_INITIALIZED,
            trigger_parser=component_trigger_to_str,
            state_parser=component_state_to_str,
        )


cdef class Component:
    """
    The base class for all system components.

    A component is not considered initialized until a message bus is registered
    (this either happens when one is passed to the constructor, or when
    registered with a trader).

    Thus, if the component does not receive a message bus through the constructor,
    then it will be in a ``PRE_INITIALIZED`` state, otherwise if one is passed
    then it will be in an ``INITIALIZED`` state.

    Parameters
    ----------
    clock : Clock
        The clock for the component.
    logger : Logger
        The logger for the component.
    trader_id : TraderId, optional
        The trader ID associated with the component.
    component_id : Identifier, optional
        The component ID. If ``None`` is passed then the identifier will be
        taken from `type(self).__name__`.
    component_name : str, optional
        The custom component name.
    msgbus : MessageBus, optional
        The message bus for the component (required before initialized).
    config : dict[str, Any], optional
        The configuration for the component.

    Raises
    ------
    ValueError
        If `component_name` is not a valid string.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        Clock clock not None,
        Logger logger not None,
        TraderId trader_id = None,
        Identifier component_id = None,
        str component_name = None,
        MessageBus msgbus = None,
        dict config = None,
    ):
        if config is None:
            config = {}
        if component_id is None:
            component_id = ComponentId(type(self).__name__)
        if component_name is None:
            component_name = component_id.value
        Condition.valid_string(component_name, "component_name")

        self.trader_id = msgbus.trader_id if msgbus is not None else None
        self.id = component_id
        self.type = type(self)

        self._msgbus = msgbus
        self._clock = clock
        self._log = LoggerAdapter(component_name=component_name, logger=logger)
        self._fsm = ComponentFSMFactory.create()
        self._config = config

        if self._msgbus is not None:
            self._initialize()

    def __eq__(self, Component other) -> bool:
        return self.id == other.id

    def __hash__(self) -> int:
        return hash(self.id)

    def __str__(self) -> str:
        return self.id.to_str()

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self.id.to_str()})"

    @classmethod
    def fully_qualified_name(cls) -> str:
        """
        Return the fully qualified name for the components class.

        Returns
        -------
        str

        References
        ----------
        https://www.python.org/dev/peps/pep-3155/

        """
        return cls.__module__ + ':' + cls.__qualname__

    @property
    def state(self) -> ComponentState:
        """
        Return the components current state.

        Returns
        -------
        ComponentState

        """
        return PyComponentState(self._fsm.state)

    @property
    def is_initialized(self) -> bool:
        """
        Return whether the component has been initialized (component.state >= ``INITIALIZED``).

        Returns
        -------
        bool

        """
        return self._fsm.state >= ComponentState.READY

    @property
    def is_running(self) -> bool:
        """
        Return whether the current component state is ``RUNNING``.

        Returns
        -------
        bool

        """
        return self._fsm.state == ComponentState.RUNNING

    @property
    def is_stopped(self) -> bool:
        """
        Return whether the current component state is ``STOPPED``.

        Returns
        -------
        bool

        """
        return self._fsm.state == ComponentState.STOPPED

    @property
    def is_disposed(self) -> bool:
        """
        Return whether the current component state is ``DISPOSED``.

        Returns
        -------
        bool

        """
        return self._fsm.state == ComponentState.DISPOSED

    @property
    def is_degraded(self) -> bool:
        """
        Return whether the current component state is ``DEGRADED``.

        Returns
        -------
        bool

        """
        return self._fsm.state == ComponentState.DEGRADED

    @property
    def is_faulted(self) -> bool:
        """
        Return whether the current component state is ``FAULTED``.

        Returns
        -------
        bool

        """
        return self._fsm.state == ComponentState.FAULTED

    cdef void _change_clock(self, Clock clock):
        Condition.not_none(clock, "clock")

        self._clock = clock

    cdef void _change_logger(self, Logger logger):
        Condition.not_none(logger, "logger")

        self._log = LoggerAdapter(component_name=self.id.value, logger=logger)

    cdef void _change_msgbus(self, MessageBus msgbus):
        # As an additional system wiring check: if a message bus is being added
        # here, then there should not be an existing trader ID or message bus.
        Condition.not_none(msgbus, "msgbus")
        Condition.none(self.trader_id, "self.trader_id")
        Condition.none(self._msgbus, "self._msgbus")

        self.trader_id = msgbus.trader_id
        self._msgbus = msgbus
        self._initialize()

# -- ABSTRACT METHODS -----------------------------------------------------------------------------

    cpdef void _start(self):
        # Optionally override in subclass
        pass

    cpdef void _stop(self):
        # Optionally override in subclass
        pass

    cpdef void _resume(self):
        # Optionally override in subclass
        pass

    cpdef void _reset(self):
        # Optionally override in subclass
        pass

    cpdef void _dispose(self):
        # Optionally override in subclass
        pass

    cpdef void _degrade(self):
        # Optionally override in subclass
        pass

    cpdef void _fault(self):
        # Optionally override in subclass
        pass

# -- COMMANDS -------------------------------------------------------------------------------------

    cdef void _initialize(self):
        # This is a protected method dependent on registration of a message bus
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.INITIALIZE,  # -> INITIALIZED
                is_transitory=False,
                action=None,
            )
        except Exception as e:
            self._log.exception(f"{repr(self)}: Error on initialize", e)
            raise

    cpdef void start(self):
        """
        Start the component.

        While executing `on_start()` any exception will be logged and reraised, then the component
        will remain in a ``STARTING`` state.

        Warnings
        --------
        Do not override.

        If the component is not in a valid state from which to execute this method,
        then the component state will not change, and an error will be logged.

        """
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.START,  # -> STARTING
                is_transitory=True,
                action=self._start,
            )
        except Exception as e:
            self._log.exception(f"{repr(self)}: Error on START", e)
            raise  # Halt state transition

        self._trigger_fsm(
            trigger=ComponentTrigger.START_COMPLETED,
            is_transitory=False,
            action=None,
        )

    cpdef void stop(self):
        """
        Stop the component.

        While executing `on_stop()` any exception will be logged and reraised, then the component
        will remain in a ``STOPPING`` state.

        Warnings
        --------
        Do not override.

        If the component is not in a valid state from which to execute this method,
        then the component state will not change, and an error will be logged.

        """
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.STOP,  # -> STOPPING
                is_transitory=True,
                action=self._stop,
            )
        except Exception as e:
            self._log.exception(f"{repr(self)}: Error on STOP", e)
            raise  # Halt state transition

        self._trigger_fsm(
            trigger=ComponentTrigger.STOP_COMPLETED,
            is_transitory=False,
            action=None,
        )

    cpdef void resume(self):
        """
        Resume the component.

        While executing `on_resume()` any exception will be logged and reraised, then the component
        will remain in a ``RESUMING`` state.

        Warnings
        --------
        Do not override.

        If the component is not in a valid state from which to execute this method,
        then the component state will not change, and an error will be logged.

        """
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.RESUME,  # -> RESUMING
                is_transitory=True,
                action=self._resume,
            )
        except Exception as e:
            self._log.exception(f"{repr(self)}: Error on RESUME", e)
            raise  # Halt state transition

        self._trigger_fsm(
            trigger=ComponentTrigger.RESUME_COMPLETED,
            is_transitory=False,
            action=None,
        )

    cpdef void reset(self):
        """
        Reset the component.

        All stateful fields are reset to their initial value.

        While executing `on_reset()` any exception will be logged and reraised, then the component
        will remain in a ``RESETTING`` state.

        Warnings
        --------
        Do not override.

        If the component is not in a valid state from which to execute this method,
        then the component state will not change, and an error will be logged.

        """
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.RESET,  # -> RESETTING
                is_transitory=True,
                action=self._reset,
            )
        except Exception as e:
            self._log.exception(f"{repr(self)}: Error on RESET", e)
            raise  # Halt state transition

        self._trigger_fsm(
            trigger=ComponentTrigger.RESET_COMPLETED,
            is_transitory=False,
            action=None,
        )

    cpdef void dispose(self):
        """
        Dispose of the component.

        While executing `on_dispose()` any exception will be logged and reraised, then the component
        will remain in a ``DISPOSING`` state.

        Warnings
        --------
        Do not override.

        If the component is not in a valid state from which to execute this method,
        then the component state will not change, and an error will be logged.

        """
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.DISPOSE,  # -> DISPOSING
                is_transitory=True,
                action=self._dispose,
            )
        except Exception as e:
            self._log.exception(f"{repr(self)}: Error on DISPOSE", e)
            raise  # Halt state transition

        self._trigger_fsm(
            trigger=ComponentTrigger.DISPOSE_COMPLETED,
            is_transitory=False,
            action=None,
        )

    cpdef void degrade(self):
        """
        Degrade the component.

        While executing `on_degrade()` any exception will be logged and reraised, then the component
        will remain in a ``DEGRADING`` state.

        Warnings
        --------
        Do not override.

        If the component is not in a valid state from which to execute this method,
        then the component state will not change, and an error will be logged.

        """
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.DEGRADE,  # -> DEGRADING
                is_transitory=True,
                action=self._degrade,
            )
        except Exception as e:
            self._log.exception(f"{repr(self)}: Error on DEGRADE", e)
            raise  # Halt state transition

        self._trigger_fsm(
            trigger=ComponentTrigger.DEGRADE_COMPLETED,
            is_transitory=False,
            action=None,
        )

    cpdef void fault(self):
        """
        Fault the component.

        Calling this method multiple times has the same effect as calling it once (it is idempotent).
        Once called, it cannot be reversed, and no other methods should be called on this instance.

        While executing `on_fault()` any exception will be logged and reraised, then the component
        will remain in a ``FAULTING`` state.

        Warnings
        --------
        Do not override.

        If the component is not in a valid state from which to execute this method,
        then the component state will not change, and an error will be logged.

        """
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.FAULT,  # -> FAULTING
                is_transitory=True,
                action=self._fault,
            )
        except Exception as e:
            self._log.exception(f"{repr(self)}: Error on FAULT", e)
            raise  # Halt state transition

        self._trigger_fsm(
            trigger=ComponentTrigger.FAULT_COMPLETED,
            is_transitory=False,
            action=None,
        )

# --------------------------------------------------------------------------------------------------

    cdef void _trigger_fsm(
        self,
        ComponentTrigger trigger,
        bint is_transitory,
        action: Optional[Callable[[None], None]] = None,
    ):
        try:
            self._fsm.trigger(trigger)
        except InvalidStateTrigger as e:
            self._log.error(f"{repr(e)} state {self._fsm.state_string_c()}.")
            return  # Guards against invalid state

        self._log.info(f"{self._fsm.state_string_c()}.{'..' if is_transitory else ''}")

        if action is not None:
            action()

        if self._fsm == ComponentState.PRE_INITIALIZED:
            return  # Cannot publish event

        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef ComponentStateChanged event = ComponentStateChanged(
            trader_id=self.trader_id,
            component_id=self.id,
            component_type=self.type.__name__,
            state=self._fsm.state,
            config=self._config,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )

        self._msgbus.publish(
            topic=f"events.system.{self.id}",
            msg=event,
        )


cdef class Throttler:
    """
    Provides a generic throttler which can either buffer or drop messages.

    Will throttle messages to the given maximum limit-interval rate.
    If an `output_drop` handler is provided, then will drop messages which
    would exceed the rate limit. Otherwise will buffer messages until within
    the rate limit, then send.

    Parameters
    ----------
    name : str
        The unique name of the throttler.
    limit : int
        The limit setting for the throttling.
    interval : timedelta
        The interval setting for the throttling.
    clock : Clock
        The clock for the throttler.
    logger : Logger
        The logger for the throttler.
    output_send : Callable[[Any], None]
        The output handler to send messages from the throttler.
    output_drop : Callable[[Any], None], optional
        The output handler to drop messages from the throttler.
        If ``None`` then messages will be buffered.

    Raises
    ------
    ValueError
        If `name` is not a valid string.
    ValueError
        If `limit` is not positive (> 0).
    ValueError
        If `interval` is not positive (> 0).
    ValueError
        If `output_send` is not of type `Callable`.
    ValueError
        If `output_drop` is not of type `Callable` or ``None``.

    Warnings
    --------
    This throttler is not thread-safe and must be called from the same thread as
    the event loop.

    The internal buffer queue is unbounded and so a bounded queue should be
    upstream.
    """

    def __init__(
        self,
        str name,
        int limit,
        timedelta interval not None,
        Clock clock not None,
        Logger logger not None,
        output_send not None: Callable[[Any], None],
        output_drop: Optional[Callable[[Any], None]] = None,
    ):
        Condition.valid_string(name, "name")
        Condition.positive_int(limit, "limit")
        Condition.positive(interval.total_seconds(), "interval.total_seconds()")
        Condition.callable(output_send, "output_send")
        Condition.callable_or_none(output_drop, "output_drop")

        self._clock = clock
        self._log = LoggerAdapter(component_name=f"Throttler-{name}", logger=logger)
        self._interval_ns = secs_to_nanos(interval.total_seconds())
        self._buffer = deque()
        self._timer_name = f"{name}-DEQUE"
        self._timestamps = deque(maxlen=limit)
        self._output_send = output_send
        self._output_drop = output_drop
        self._warm = False  # If throttler has sent at least limit number of msgs

        self.name = name
        self.limit = limit
        self.interval = interval
        self.is_limiting = False
        self.recv_count = 0
        self.sent_count = 0

        self._log.info("READY.")

    @property
    def qsize(self):
        """
        Return the qsize of the internal buffer.

        Returns
        -------
        int

        """
        return len(self._buffer)

    cpdef double used(self):
        """
        Return the percentage of maximum rate currently used.

        Returns
        -------
        double
            [0, 1.0].

        """
        if not self._warm:
            if self.sent_count < 2:
                return 0

        cdef int64_t spread = self._clock.timestamp_ns() - self._timestamps[-1]
        cdef int64_t diff = max_uint64(0, self._interval_ns - spread)
        cdef double used = <double>diff / <double>self._interval_ns

        if not self._warm:
            used *= <double>self.sent_count / <double>self.limit

        return used

    cpdef void send(self, msg):
        """
        Send the given message through the throttler.

        Parameters
        ----------
        msg : object
            The message to send.

        """
        self.recv_count += 1

        # Throttling is active
        if self.is_limiting:
            self._limit_msg(msg)
            return

        # Check msg rate
        cdef int64_t delta_next = self._delta_next()
        if delta_next <= 0:
            self._send_msg(msg)
        else:
            # Start throttling
            self._limit_msg(msg)

    cdef int64_t _delta_next(self):
        if not self._warm:
            if self.sent_count < self.limit:
                return 0
            self._warm = True

        cdef int64_t diff = self._timestamps[0] - self._timestamps[-1]
        return self._interval_ns - diff

    cdef void _limit_msg(self, msg):
        if self._output_drop is None:
            # Buffer
            self._buffer.appendleft(msg)
            timer_target = self._process
            self._log.warning(f"Buffering {msg}.")
        else:
            # Drop
            self._output_drop(msg)
            timer_target = self._resume
            self._log.warning(f"Dropped {msg}.")

        if not self.is_limiting:
            self._set_timer(timer_target)
            self.is_limiting = True

    cdef void _set_timer(self, handler: Callable[[TimeEvent], None]):
        # Cancel any existing timer
        if self._timer_name in self._clock.timer_names:
            self._clock.cancel_timer(self._timer_name)

        self._clock.set_time_alert_ns(
            name=self._timer_name,
            alert_time_ns=self._clock.timestamp_ns() + self._delta_next(),
            callback=handler,
        )

    cpdef void _process(self, TimeEvent event):
        # Send next msg on buffer
        msg = self._buffer.pop()
        self._send_msg(msg)

        # Send remaining messages if within rate
        cdef int64_t delta_next
        while self._buffer:
            delta_next = self._delta_next()
            msg = self._buffer.pop()
            if delta_next <= 0:
                self._send_msg(msg)
            else:
                self._set_timer(self._process)
                return

        # No longer throttling
        self.is_limiting = False

    cpdef void _resume(self, TimeEvent event):
        self.is_limiting = False

    cdef void _send_msg(self, msg):
        self._timestamps.appendleft(self._clock.timestamp_ns())
        self._output_send(msg)
        self.sent_count += 1


cdef inline uint64_t max_uint64(uint64_t a, uint64_t b):
    if a > b:
        return a
    else:
        return b
