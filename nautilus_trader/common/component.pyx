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

import copy
from collections import deque
from typing import Any
from typing import Callable
from typing import Optional

import cython
import msgspec
import numpy as np

from nautilus_trader.config.error import InvalidConfiguration
from nautilus_trader.core.rust.common import ComponentState as PyComponentState

cimport numpy as np
from cpython.datetime cimport timedelta
from libc.stdint cimport int64_t
from libc.stdint cimport uint64_t

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.clock cimport TimeEvent
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.messages cimport ComponentStateChanged
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.fsm cimport FiniteStateMachine
from nautilus_trader.core.fsm cimport InvalidStateTrigger
from nautilus_trader.core.rust.common cimport ComponentState
from nautilus_trader.core.rust.common cimport ComponentTrigger
from nautilus_trader.core.rust.common cimport component_state_from_cstr
from nautilus_trader.core.rust.common cimport component_state_to_cstr
from nautilus_trader.core.rust.common cimport component_trigger_from_cstr
from nautilus_trader.core.rust.common cimport component_trigger_to_cstr
from nautilus_trader.core.rust.common cimport msgbus_drop
from nautilus_trader.core.rust.common cimport msgbus_new
from nautilus_trader.core.rust.common cimport msgbus_publish_external
from nautilus_trader.core.rust.core cimport secs_to_nanos
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.core.string cimport pybytes_to_cstr
from nautilus_trader.core.string cimport pystr_to_cstr
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.identifiers cimport ComponentId
from nautilus_trader.model.identifiers cimport Identifier
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.serialization.base cimport EXTERNAL_PUBLISHING_TYPES
from nautilus_trader.serialization.base cimport Serializer


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


cdef class MessageBus:
    """
    Provides a generic message bus to facilitate various messaging patterns.

    The bus provides both a producer and consumer API for Pub/Sub, Req/Rep, as
    well as direct point-to-point messaging to registered endpoints.

    Pub/Sub wildcard patterns for hierarchical topics are possible:
     - `*` asterisk represents one or more characters in a pattern.
     - `?` question mark represents a single character in a pattern.

    Given a topic and pattern potentially containing wildcard characters, i.e.
    `*` and `?`, where `?` can match any single character in the topic, and `*`
    can match any number of characters including zero characters.

    The asterisk in a wildcard matches any character zero or more times. For
    example, `comp*` matches anything beginning with `comp` which means `comp`,
    `complete`, and `computer` are all matched.

    A question mark matches a single character once. For example, `c?mp` matches
    `camp` and `comp`. The question mark can also be used more than once.
    For example, `c??p` would match both of the above examples and `coop`.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID associated with the message bus.
    clock : Clock
        The clock for the message bus.
    logger : Logger
        The logger for the message bus.
    name : str, optional
        The custom name for the message bus.
    serializer : Serializer, optional
        The serializer for database operations.
    snapshot_orders : bool, default False
        If order state snapshots should be published externally.
    snapshot_positions : bool, default False
        If position state snapshots should be published externally.
    config : MessageBusConfig, optional
        The configuration for the message bus.

    Raises
    ------
    ValueError
        If `name` is not ``None`` and not a valid string.

    Warnings
    --------
    This message bus is not thread-safe and must be called from the same thread
    as the event loop.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        Clock clock not None,
        Logger logger not None,
        UUID4 instance_id = None,
        str name = None,
        Serializer serializer = None,
        bint snapshot_orders: bool = False,
        bint snapshot_positions: bool = False,
        config: Any | None = None,
    ):
        # Temporary fix for import error
        from nautilus_trader.config.common import MessageBusConfig

        if instance_id is None:
            instance_id = UUID4()
        if name is None:
            name = type(self).__name__
        Condition.valid_string(name, "name")
        if config is None:
            config = MessageBusConfig()
        Condition.type(config, MessageBusConfig, "config")

        self.trader_id = trader_id
        self.serializer = serializer
        self.has_backing = config.database is not None
        self.snapshot_orders = snapshot_orders
        self.snapshot_positions = snapshot_positions

        self._clock = clock
        self._log = LoggerAdapter(component_name=name, logger=logger)

        # Validate configuration
        if config.buffer_interval_ms and config.buffer_interval_ms > 1000:
            self._log.warning(
                f"High `buffer_interval_ms` at {config.buffer_interval_ms}, "
                "recommended range is [10, 1000] milliseconds.",
            )

        if (snapshot_orders or snapshot_positions) and not config.stream:
            raise InvalidConfiguration(
                "Invalid `MessageBusConfig`: Cannot configure snapshots without providing a `stream` name. "
                "This is because currently the message bus will write to the same snapshot keys as the cache.",
            )

        # Configuration
        self._log.info(f"{config.database=}", LogColor.BLUE)
        self._log.info(f"{config.encoding=}", LogColor.BLUE)
        self._log.info(f"{config.timestamps_as_iso8601=}", LogColor.BLUE)
        self._log.info(f"{config.buffer_interval_ms=}", LogColor.BLUE)
        self._log.info(f"{config.autotrim_mins=}", LogColor.BLUE)
        self._log.info(f"{config.stream=}", LogColor.BLUE)
        self._log.info(f"{config.use_instance_id=}", LogColor.BLUE)
        self._log.info(f"{config.types_filter=}", LogColor.BLUE)

        # Copy and clear `types_filter` before passing down to the core MessageBus
        cdef list types_filter = copy.copy(config.types_filter)
        if config.types_filter is not None:
            config.types_filter.clear()

        self._mem = msgbus_new(
            pystr_to_cstr(trader_id.value),
            pystr_to_cstr(name) if name else NULL,
            pystr_to_cstr(instance_id.to_str()),
            pybytes_to_cstr(msgspec.json.encode(config)),
        )

        self._endpoints: dict[str, Callable[[Any], None]] = {}
        self._patterns: dict[str, Subscription[:]] = {}
        self._subscriptions: dict[Subscription, list[str]] = {}
        self._correlation_index: dict[UUID4, Callable[[Any], None]] = {}
        self._has_backing = config.database is not None
        self._publishable_types = EXTERNAL_PUBLISHING_TYPES
        if types_filter is not None:
            self._publishable_types = tuple(o for o in EXTERNAL_PUBLISHING_TYPES if o not in types_filter)

        # Counters
        self.sent_count = 0
        self.req_count = 0
        self.res_count = 0
        self.pub_count = 0

    def __del__(self) -> None:
        if self._mem._0 != NULL:
            msgbus_drop(self._mem)

    cpdef list endpoints(self):
        """
        Return all endpoint addresses registered with the message bus.

        Returns
        -------
        list[str]

        """
        return list(self._endpoints.keys())

    cpdef list topics(self):
        """
        Return all topics with active subscribers.

        Returns
        -------
        list[str]

        """
        return sorted(set([s.topic for s in self._subscriptions.keys()]))

    cpdef list subscriptions(self, str pattern = None):
        """
        Return all subscriptions matching the given topic `pattern`.

        Parameters
        ----------
        pattern : str, optional
            The topic pattern filter. May include wildcard characters `*` and `?`.
            If ``None`` then query is for **all** topics.

        Returns
        -------
        list[Subscription]

        """
        if pattern is None:
            pattern = "*"  # Wildcard
        Condition.valid_string(pattern, "pattern")

        return [s for s in self._subscriptions if is_matching(s.topic, pattern)]

    cpdef bint has_subscribers(self, str pattern = None):
        """
        If the message bus has subscribers for the give topic `pattern`.

        Parameters
        ----------
        pattern : str, optional
            The topic filter. May include wildcard characters `*` and `?`.
            If ``None`` then query is for **all** topics.

        Returns
        -------
        bool

        """
        return len(self.subscriptions(pattern)) > 0

    cpdef bint is_subscribed(self, str topic, handler: Callable[[Any], None]):
        """
        Return if topic and handler is subscribed to the message bus.

        Does not consider any previous `priority`.

        Parameters
        ----------
        topic : str
            The topic of the subscription.
        handler : Callable[[Any], None]
            The handler of the subscription.

        Returns
        -------
        bool

        """
        Condition.valid_string(topic, "topic")
        Condition.callable(handler, "handler")

        # Create subscription
        cdef Subscription sub = Subscription(
            topic=topic,
            handler=handler,
        )

        return sub in self._subscriptions

    cpdef bint is_pending_request(self, UUID4 request_id):
        """
        Return if the given `request_id` is still pending a response.

        Parameters
        ----------
        request_id : UUID4
            The request ID to check (to match the correlation_id).

        Returns
        -------
        bool

        """
        Condition.not_none(request_id, "request_id")

        return request_id in self._correlation_index

    cpdef void register(self, str endpoint, handler: Callable[[Any], None]):
        """
        Register the given `handler` to receive messages at the `endpoint` address.

        Parameters
        ----------
        endpoint : str
            The endpoint address to register.
        handler : Callable[[Any], None]
            The handler for the registration.

        Raises
        ------
        ValueError
            If `endpoint` is not a valid string.
        ValueError
            If `handler` is not of type `Callable`.
        KeyError
            If `endpoint` already registered.

        """
        Condition.valid_string(endpoint, "endpoint")
        Condition.callable(handler, "handler")
        Condition.not_in(endpoint, self._endpoints, "endpoint", "_endpoints")

        self._endpoints[endpoint] = handler

        self._log.debug(f"Added endpoint '{endpoint}' {handler}.")

    cpdef void deregister(self, str endpoint, handler: Callable[[Any], None]):
        """
        Deregister the given `handler` from the `endpoint` address.

        Parameters
        ----------
        endpoint : str
            The endpoint address to deregister.
        handler : Callable[[Any], None]
            The handler to deregister.

        Raises
        ------
        ValueError
            If `endpoint` is not a valid string.
        ValueError
            If `handler` is not of type `Callable`.
        KeyError
            If `endpoint` is not registered.
        ValueError
            If `handler` is not registered at the endpoint.

        """
        Condition.valid_string(endpoint, "endpoint")
        Condition.callable(handler, "handler")
        Condition.is_in(endpoint, self._endpoints, "endpoint", "self._endpoints")
        Condition.equal(handler, self._endpoints[endpoint], "handler", "self._endpoints[endpoint]")

        del self._endpoints[endpoint]

        self._log.debug(f"Removed endpoint '{endpoint}' {handler}.")

    cpdef void send(self, str endpoint, msg: Any):
        """
        Send the given message to the given `endpoint` address.

        Parameters
        ----------
        endpoint : str
            The endpoint address to send the message to.
        msg : object
            The message to send.

        """
        Condition.not_none(endpoint, "endpoint")
        Condition.not_none(msg, "msg")

        handler = self._endpoints.get(endpoint)
        if handler is None:
            self._log.error(
                f"Cannot send message: no endpoint registered at '{endpoint}'.",
            )
            return  # Cannot send

        handler(msg)
        self.sent_count += 1

    cpdef void request(self, str endpoint, Request request):
        """
        Handle the given `request`.

        Will log an error if the correlation ID already exists.

        Parameters
        ----------
        endpoint : str
            The endpoint address to send the request to.
        request : Request
            The request to handle.

        """
        Condition.not_none(endpoint, "endpoint")
        Condition.not_none(request, "request")

        if request.id in self._correlation_index:
            self._log.error(
                f"Cannot handle request: "
                f"duplicate ID {request.id} found in correlation index.",
            )
            return  # Do not handle duplicates

        self._correlation_index[request.id] = request.callback

        handler = self._endpoints.get(endpoint)
        if handler is None:
            self._log.error(
                f"Cannot handle request: no endpoint registered at '{endpoint}'.",
            )
            return  # Cannot handle

        handler(request)
        self.req_count += 1

    cpdef void response(self, Response response):
        """
        Handle the given `response`.

        Will log an error if the correlation ID is not found.

        Parameters
        ----------
        response : Response
            The response to handle

        """
        Condition.not_none(response, "response")

        callback = self._correlation_index.pop(response.correlation_id, None)
        if callback is None:
            self._log.error(
                f"Cannot handle response: "
                f"callback not found for correlation_id {response.correlation_id}.",
            )
            return  # Cannot handle

        callback(response)
        self.res_count += 1

    cpdef void subscribe(
        self,
        str topic,
        handler: Callable[[Any], None],
        int priority = 0,
    ):
        """
        Subscribe to the given message `topic` with the given callback `handler`.

        Parameters
        ----------
        topic : str
            The topic for the subscription. May include wildcard characters
            `*` and `?`.
        handler : Callable[[Any], None]
            The handler for the subscription.
        priority : int, optional
            The priority for the subscription. Determines the ordering of
            handlers receiving messages being processed, higher priority
            handlers will receive messages prior to lower priority handlers.

        Raises
        ------
        ValueError
            If `topic` is not a valid string.
        ValueError
            If `handler` is not of type `Callable`.

        Warnings
        --------
        Assigning priority handling is an advanced feature which *shouldn't
        normally be needed by most users*. **Only assign a higher priority to the
        subscription if you are certain of what you're doing**. If an inappropriate
        priority is assigned then the handler may receive messages before core
        system components have been able to process necessary calculations and
        produce potential side effects for logically sound behavior.

        """
        Condition.valid_string(topic, "topic")
        Condition.callable(handler, "handler")

        # Create subscription
        cdef Subscription sub = Subscription(
            topic=topic,
            handler=handler,
            priority=priority,
        )

        # Check if already exists
        if sub in self._subscriptions:
            self._log.debug(f"{sub} already exists.")
            return

        cdef list matches = []
        cdef list patterns = list(self._patterns.keys())

        cdef str pattern
        cdef list subs
        for pattern in patterns:
            if is_matching(topic, pattern):
                subs = list(self._patterns[pattern])
                subs.append(sub)
                subs = sorted(subs, reverse=True)
                self._patterns[pattern] = np.ascontiguousarray(subs, dtype=Subscription)
                matches.append(pattern)

        self._subscriptions[sub] = sorted(matches)

        self._log.debug(f"Added {sub}.")

    cpdef void unsubscribe(self, str topic, handler: Callable[[Any], None]):
        """
        Unsubscribe the given callback `handler` from the given message `topic`.

        Parameters
        ----------
        topic : str, optional
            The topic to unsubscribe from. May include wildcard characters `*`
            and `?`.
        handler : Callable[[Any], None]
            The handler for the subscription.

        Raises
        ------
        ValueError
            If `topic` is not a valid string.
        ValueError
            If `handler` is not of type `Callable`.

        """
        Condition.valid_string(topic, "topic")
        Condition.callable(handler, "handler")

        cdef Subscription sub = Subscription(topic=topic, handler=handler)

        cdef list patterns = self._subscriptions.get(sub)

        # Check if exists
        if patterns is None:
            self._log.warning(f"{sub} not found.")
            return

        cdef str pattern
        for pattern in patterns:
            subs = list(self._patterns[pattern])
            subs.remove(sub)
            subs = sorted(subs, reverse=True)
            self._patterns[pattern] = np.ascontiguousarray(subs, dtype=Subscription)

        del self._subscriptions[sub]

        self._log.debug(f"Removed {sub}.")

    cpdef void publish(self, str topic, msg: Any):
        """
        Publish the given message for the given `topic`.

        Subscription handlers will receive the message in priority order
        (highest first).

        Parameters
        ----------
        topic : str
            The topic to publish on.
        msg : object
            The message to publish.

        """
        self.publish_c(topic, msg)

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cdef void publish_c(self, str topic, msg: Any):
        Condition.not_none(topic, "topic")
        Condition.not_none(msg, "msg")

        # Get all subscriptions matching topic pattern
        cdef Subscription[:] subs = self._patterns.get(topic)
        if subs is None:
            # Add the topic pattern and get matching subscribers
            subs = self._resolve_subscriptions(topic)

        # Send message to all matched subscribers
        cdef:
            int i
            Subscription sub
        for i in range(len(subs)):
            sub = subs[i]
            sub.handler(msg)

        # Publish externally (if configured)
        cdef bytes payload_bytes
        if self._has_backing and self.serializer is not None:
            if isinstance(msg, self._publishable_types):
                if isinstance(msg, bytes):
                    payload_bytes = msg
                else:
                    payload_bytes = self.serializer.serialize(msg)
                msgbus_publish_external(
                    &self._mem,
                    pystr_to_cstr(topic),
                    pybytes_to_cstr(payload_bytes),
                )

        self.pub_count += 1

    cdef Subscription[:] _resolve_subscriptions(self, str topic):
        cdef list subs_list = []
        cdef Subscription existing_sub
        for existing_sub in self._subscriptions:
            if is_matching(topic, existing_sub.topic):
                subs_list.append(existing_sub)

        subs_list = sorted(subs_list, reverse=True)
        cdef Subscription[:] subs_array = np.ascontiguousarray(subs_list, dtype=Subscription)
        self._patterns[topic] = subs_array

        cdef list matches
        for sub in subs_array:
            matches = self._subscriptions.get(sub, [])
            if topic not in matches:
                matches.append(topic)
            self._subscriptions[sub] = sorted(matches)

        return subs_array


cdef inline bint is_matching(str topic, str pattern):
    # Get length of string and wildcard pattern
    cdef int n = len(topic)
    cdef int m = len(pattern)

    # Create a DP lookup table
    cdef np.ndarray[np.int8_t, ndim=2] t = np.empty((n + 1, m + 1), dtype=np.int8)
    t.fill(False)

    # If both pattern and string are empty: match
    t[0, 0] = True

    # Handle empty string case (i == 0)
    cdef int j
    for j in range(1, m + 1):
        if pattern[j - 1] == '*':
            t[0, j] = t[0, j - 1]

    # Build a matrix in a bottom-up manner
    cdef int i
    for i in range(1, n + 1):
        for j in range(1, m + 1):
            if pattern[j - 1] == '*':
                t[i, j] = t[i - 1, j] or t[i, j - 1]
            elif pattern[j - 1] == '?' or topic[i - 1] == pattern[j - 1]:
                t[i, j] = t[i - 1, j - 1]

    return t[n, m]


# Python wrapper for test access
def is_matching_py(str topic, str pattern) -> bool:
    return is_matching(topic, pattern)


cdef class Subscription:
    """
    Represents a subscription to a particular topic.

    This is an internal class intended to be used by the message bus to organize
    topics and their subscribers.

    Parameters
    ----------
    topic : str
        The topic for the subscription. May include wildcard characters `*` and `?`.
    handler : Callable[[Message], None]
        The handler for the subscription.
    priority : int
        The priority for the subscription.

    Raises
    ------
    ValueError
        If `topic` is not a valid string.
    ValueError
        If `handler` is not of type `Callable`.
    ValueError
        If `priority` is negative (< 0).

    Notes
    -----
    The subscription equality is determined by the topic and handler,
    priority is not considered (and could change).
    """

    def __init__(
        self,
        str topic,
        handler not None: Callable[[Any], None],
        int priority=0,
    ):
        Condition.valid_string(topic, "topic")
        Condition.callable(handler, "handler")
        Condition.not_negative_int(priority, "priority")

        self.topic = topic
        self.handler = handler
        self.priority = priority

    def __eq__(self, Subscription other) -> bool:
        return self.topic == other.topic and self.handler == other.handler

    def __lt__(self, Subscription other) -> bool:
        return self.priority < other.priority

    def __le__(self, Subscription other) -> bool:
        return self.priority <= other.priority

    def __gt__(self, Subscription other) -> bool:
        return self.priority > other.priority

    def __ge__(self, Subscription other) -> bool:
        return self.priority >= other.priority

    def __hash__(self) -> int:
        # Convert handler to string to avoid builtin_function_or_method hashing issues
        return hash((self.topic, str(self.handler)))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"topic={self.topic}, "
            f"handler={self.handler}, "
            f"priority={self.priority})"
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
